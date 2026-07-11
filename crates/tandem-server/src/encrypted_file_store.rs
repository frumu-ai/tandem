use std::path::Path;

use anyhow::Context;
#[cfg(test)]
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use tandem_enterprise_contract::DataClass;
use tandem_memory::envelope::{MemoryEnvelopeAuthority, MemoryEnvelopeMetadata, MemoryKeyScope};
use tandem_memory::types::MemoryTenantScope;
use tandem_memory::{MemoryCryptoProvider, MemoryDecryptPrincipal, MemoryDecryptPurpose};
use tokio::fs;

mod integrity;

pub(crate) use integrity::{
    append_jsonl_record_file, read_jsonl_records_file, read_text_file, write_json_records_file,
};

pub(crate) const ENCRYPTED_PAYLOAD_PREFIX: &str = "tce1:";
pub(crate) const SCOPED_RECORD_PREFIX: &str = "tgs1:";
pub(crate) const SCOPED_COLLECTION_PREFIX: &str = "tgsc1:";
pub(crate) const AUTHENTICATED_COLLECTION_PREFIX: &str = "tgsc2:";
pub(crate) const AUTHENTICATED_JSONL_PREFIX: &str = "tgj2:";

const SCOPED_RECORD_VERSION: u32 = 2;
const AUTHENTICATED_STORE_VERSION: u32 = 2;
const MEMORY_DECRYPT_PRINCIPAL_ENV: &str = "TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProtectedRecordContext {
    pub(crate) key_scope: MemoryKeyScope,
    pub(crate) policy_decision_id: String,
    pub(crate) audit_id: String,
}

impl ProtectedRecordContext {
    pub(crate) fn new(
        key_scope: MemoryKeyScope,
        policy_decision_id: impl Into<String>,
        audit_id: impl Into<String>,
    ) -> Self {
        Self {
            key_scope,
            policy_decision_id: policy_decision_id.into(),
            audit_id: audit_id.into(),
        }
    }

    fn authority(&self) -> MemoryEnvelopeAuthority {
        MemoryEnvelopeAuthority::new(
            self.key_scope.clone(),
            self.policy_decision_id.clone(),
            self.audit_id.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProtectedStoreContext {
    pub(crate) store_id: String,
    pub(crate) manifest: ProtectedRecordContext,
    pub(crate) head: ProtectedRecordContext,
}

impl ProtectedStoreContext {
    pub(crate) fn new(
        store_id: impl Into<String>,
        manifest: ProtectedRecordContext,
        head: ProtectedRecordContext,
    ) -> Self {
        Self {
            store_id: store_id.into(),
            manifest,
            head,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProtectedJsonRecord {
    key: String,
    value: Value,
    context: ProtectedRecordContext,
}

impl ProtectedJsonRecord {
    pub(crate) fn new<T>(
        key: impl Into<String>,
        value: &T,
        context: ProtectedRecordContext,
    ) -> anyhow::Result<Self>
    where
        T: Serialize,
    {
        Ok(Self {
            key: key.into(),
            value: serde_json::to_value(value)?,
            context,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScopedEncryptedRecord {
    version: u32,
    ciphertext: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    envelope: Option<MemoryEnvelopeMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScopeBoundPlaintext {
    context: ProtectedRecordContext,
    payload: String,
}

#[derive(Clone)]
struct ProtectedFileCrypto {
    provider: MemoryCryptoProvider,
    principal_id: Option<String>,
}

impl ProtectedFileCrypto {
    fn from_env() -> Self {
        Self {
            provider: MemoryCryptoProvider::from_env(),
            principal_id: std::env::var(MEMORY_DECRYPT_PRINCIPAL_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        }
    }

    fn validate_context(context: &ProtectedRecordContext) -> anyhow::Result<()> {
        context
            .key_scope
            .validate_for_envelope()
            .context("validate protected file-store key scope")?;
        anyhow::ensure!(
            !context.policy_decision_id.trim().is_empty(),
            "protected file-store encryption requires a policy decision id"
        );
        anyhow::ensure!(
            !context.audit_id.trim().is_empty(),
            "protected file-store encryption requires an audit id"
        );
        Ok(())
    }

    fn encrypt_record(
        &self,
        plaintext: &str,
        context: &ProtectedRecordContext,
    ) -> anyhow::Result<String> {
        Self::validate_context(context)?;
        if self.provider.is_plaintext() {
            return Ok(plaintext.to_string());
        }

        let bound_plaintext = serde_json::to_string(&ScopeBoundPlaintext {
            context: context.clone(),
            payload: plaintext.to_string(),
        })?;
        let (ciphertext, envelope) = self
            .provider
            .encrypt_field_scoped(
                &bound_plaintext,
                &context.key_scope,
                &context.policy_decision_id,
                &context.audit_id,
            )
            .context("encrypt scoped protected file-store payload")?;

        if let Some(envelope) = envelope.as_ref() {
            envelope
                .validate_cryptographic_binding(&context.authority())
                .context("validate protected file-store envelope authority")?;
        }
        let record = ScopedEncryptedRecord {
            version: SCOPED_RECORD_VERSION,
            ciphertext,
            envelope,
        };
        Ok(format!(
            "{SCOPED_RECORD_PREFIX}{}",
            serde_json::to_string(&record)?
        ))
    }

    fn decrypt_record(
        &self,
        stored: &str,
        expected: &ProtectedRecordContext,
    ) -> anyhow::Result<String> {
        Self::validate_context(expected)?;
        if let Some(encoded) = stored.strip_prefix(SCOPED_RECORD_PREFIX) {
            let record = serde_json::from_str::<ScopedEncryptedRecord>(encoded)
                .context("parse scoped protected file-store envelope")?;
            anyhow::ensure!(
                record.version == SCOPED_RECORD_VERSION,
                "unsupported protected file-store envelope version {}",
                record.version
            );
            anyhow::ensure!(
                !self.provider.is_plaintext(),
                "scoped protected file-store payload requires a configured decrypt provider"
            );

            let envelope = record.envelope.as_ref();
            let principal = if self.provider.is_hosted() {
                let principal_id = self.principal_id.as_ref().context(
                    "scoped protected file-store decrypt requires a configured runtime principal",
                )?;
                Some(MemoryDecryptPrincipal {
                    principal_id: principal_id.clone(),
                    purpose: MemoryDecryptPurpose::RuntimeWorker,
                    tenant_scope: MemoryTenantScope {
                        org_id: expected.key_scope.org_id.clone(),
                        workspace_id: expected.key_scope.workspace_id.clone(),
                        deployment_id: expected.key_scope.deployment_id.clone(),
                    },
                    allowed_data_classes: vec![expected.key_scope.data_class],
                    allowed_source_binding_ids: expected
                        .key_scope
                        .source_binding_id
                        .iter()
                        .cloned()
                        .collect(),
                    allowed_owner_subjects: expected
                        .key_scope
                        .owner_subject
                        .iter()
                        .cloned()
                        .collect(),
                })
            } else {
                anyhow::ensure!(
                    envelope.is_none(),
                    "hosted protected file-store envelope cannot be read by a local-key provider"
                );
                None
            };
            let plaintext = self
                .provider
                .decrypt_field_scoped_authorized(
                    &record.ciphertext,
                    envelope,
                    principal.as_ref(),
                    &expected.authority(),
                    None,
                )
                .context("decrypt authorized protected file-store payload")?;
            let bound = serde_json::from_str::<ScopeBoundPlaintext>(&plaintext)
                .context("parse scope-bound protected file-store plaintext")?;
            anyhow::ensure!(
                bound.context == *expected,
                "protected file-store plaintext authority does not match trusted expected context"
            );
            return Ok(bound.payload);
        }

        anyhow::ensure!(
            !self.provider.is_hosted(),
            "hosted protected file-store legacy payload lacks authenticated expected authority"
        );
        if self.provider.is_plaintext() && is_legacy_encrypted_payload(stored) {
            anyhow::bail!(
                "encrypted protected file-store payload requires a configured decrypt provider"
            );
        }
        self.provider
            .decrypt_field_scoped(stored, None, None, None)
            .context("decrypt legacy protected file-store payload")
    }

    fn decrypt_legacy_record(&self, stored: &str) -> anyhow::Result<String> {
        anyhow::ensure!(
            !self.provider.is_hosted(),
            "hosted protected file-store legacy payload lacks authenticated expected authority"
        );
        anyhow::ensure!(
            !stored.starts_with(SCOPED_RECORD_PREFIX),
            "legacy hosted protected record cannot be trusted without an authenticated manifest"
        );
        if self.provider.is_plaintext() && is_legacy_encrypted_payload(stored) {
            anyhow::bail!(
                "encrypted protected file-store payload requires a configured decrypt provider"
            );
        }
        self.provider
            .decrypt_field_scoped(stored, None, None, None)
            .context("decrypt legacy protected file-store payload")
    }
}

fn crypto() -> ProtectedFileCrypto {
    #[cfg(test)]
    {
        if let Ok(provider) = TEST_CRYPTO.try_with(Clone::clone) {
            return provider;
        }
    }
    ProtectedFileCrypto::from_env()
}

fn is_legacy_encrypted_payload(stored: &str) -> bool {
    stored.trim_start().starts_with(ENCRYPTED_PAYLOAD_PREFIX)
}

pub(crate) fn is_encrypted_payload(stored: &str) -> bool {
    let stored = stored.trim_start();
    stored.starts_with(ENCRYPTED_PAYLOAD_PREFIX)
        || stored.starts_with(SCOPED_RECORD_PREFIX)
        || stored.starts_with(SCOPED_COLLECTION_PREFIX)
        || stored.starts_with(AUTHENTICATED_COLLECTION_PREFIX)
        || stored.starts_with(AUTHENTICATED_JSONL_PREFIX)
}

pub(crate) fn encrypt_text(
    plaintext: &str,
    context: &ProtectedRecordContext,
) -> anyhow::Result<String> {
    crypto().encrypt_record(plaintext, context)
}

pub(crate) fn decrypt_text(
    stored: &str,
    expected: &ProtectedRecordContext,
) -> anyhow::Result<String> {
    crypto().decrypt_record(stored.trim(), expected)
}

pub(crate) fn validate_hosted_crypto_ready(context: &ProtectedRecordContext) -> anyhow::Result<()> {
    let crypto = crypto();
    anyhow::ensure!(
        crypto.provider.is_hosted(),
        "hosted governance encryption is required but the KMS provider is unavailable"
    );
    let probe = crypto
        .encrypt_record("tandem-governance-kms-readiness", context)
        .context("seal hosted governance KMS readiness probe")?;
    crypto.provider.clear_hosted_dek_cache();
    let plaintext = crypto
        .decrypt_record(&probe, context)
        .context("unseal hosted governance KMS readiness probe")?;
    anyhow::ensure!(
        plaintext == "tandem-governance-kms-readiness",
        "hosted governance KMS readiness probe did not round-trip"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) async fn write_text_file(
    path: &Path,
    plaintext: &str,
    context: &ProtectedRecordContext,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let stored = encrypt_text(plaintext, context)?;
    integrity::atomic_replace(path, stored.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
pub(crate) async fn read_json_file<T>(
    path: &Path,
    expected: &ProtectedRecordContext,
) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let stored = fs::read_to_string(path).await?;
    let plaintext = decrypt_text(&stored, expected)?;
    serde_json::from_str(&plaintext).with_context(|| {
        format!(
            "parse protected file-store JSON payload from {}",
            path.display()
        )
    })
}

#[cfg(test)]
pub(crate) async fn write_json_file<T>(
    path: &Path,
    value: &T,
    context: &ProtectedRecordContext,
) -> anyhow::Result<()>
where
    T: Serialize,
{
    let plaintext = serde_json::to_string_pretty(value)?;
    write_text_file(path, &plaintext, context).await
}

#[cfg(test)]
tokio::task_local! {
    static TEST_CRYPTO: ProtectedFileCrypto;
}

#[cfg(test)]
pub(crate) async fn with_test_crypto_provider<F, T>(
    provider: MemoryCryptoProvider,
    principal_id: Option<&str>,
    future: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    TEST_CRYPTO
        .scope(
            ProtectedFileCrypto {
                provider,
                principal_id: principal_id.map(ToOwned::to_owned),
            },
            future,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use tandem_memory::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig};
    use tandem_memory::dek_cache::MemoryDekCache;
    use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
    use tandem_memory::kms_providers::{
        GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
        GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
    };
    use tandem_memory::types::{MemoryError, MemoryResult};

    const PROVIDER_ID: &str = "google_cloud_kms";
    const RUNTIME_PRINCIPAL: &str = "runtime-tandem";
    const KEK_ID: &str = "projects/test/locations/global/keyRings/tandem/cryptoKeys/governance";

    #[derive(Clone)]
    struct XorFixtureKms {
        fingerprint: u8,
        fail_encrypt: bool,
        fail_decrypt: bool,
    }

    impl GoogleCloudKmsEncryptClient for XorFixtureKms {
        fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
            if self.fail_encrypt {
                return Err(MemoryError::InvalidConfig(
                    "fixture KMS encrypt unavailable".to_string(),
                ));
            }
            assert!(!request.additional_authenticated_data.is_empty());
            let mut wrapped = Sha256::digest(&request.additional_authenticated_data).to_vec();
            wrapped.extend(request.plaintext.iter().map(|byte| byte ^ self.fingerprint));
            Ok(wrapped)
        }
    }

    impl GoogleCloudKmsDecryptClient for XorFixtureKms {
        fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
            if self.fail_decrypt {
                return Err(MemoryError::InvalidConfig(
                    "fixture KMS decrypt unavailable".to_string(),
                ));
            }
            assert!(!request.additional_authenticated_data.is_empty());
            let expected = Sha256::digest(&request.additional_authenticated_data);
            if request.ciphertext.len() < expected.len() {
                return Err(MemoryError::InvalidConfig(
                    "fixture KMS ciphertext is truncated".to_string(),
                ));
            }
            let (actual, ciphertext) = request.ciphertext.split_at(expected.len());
            if actual != &expected[..] {
                return Err(MemoryError::InvalidConfig(
                    "fixture KMS additional authenticated data mismatch".to_string(),
                ));
            }
            Ok(ciphertext
                .iter()
                .map(|byte| byte ^ self.fingerprint)
                .collect())
        }
    }

    fn hosted_provider(kms: XorFixtureKms) -> MemoryCryptoProvider {
        let config =
            MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).expect("config");
        let broker = MemoryDecryptBroker::new(config).expect("broker");
        let wrap = GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL)
            .expect("wrap provider");
        let unwrap =
            GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).expect("unwrap provider");
        MemoryCryptoProvider::hosted(HostedMemoryEnvelopeCrypto::new(
            broker,
            Box::new(wrap),
            Box::new(unwrap),
            MemoryDekCache::new(64),
            PROVIDER_ID,
            RUNTIME_PRINCIPAL,
            KEK_ID,
            "1",
            0,
        ))
    }

    fn tenant(org: &str) -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: org.to_string(),
            workspace_id: "hq".to_string(),
            deployment_id: Some("prod".to_string()),
        }
    }

    fn context(org: &str, department: &str, record_id: &str) -> ProtectedRecordContext {
        ProtectedRecordContext::new(
            MemoryKeyScope::new(
                &tenant(org),
                DataClass::Restricted,
                Some("tandem-governance-test".to_string()),
            )
            .with_org_unit(Some(department.to_string())),
            "governance-test-policy",
            record_id,
        )
    }

    fn healthy_kms() -> XorFixtureKms {
        XorFixtureKms {
            fingerprint: 0x5a,
            fail_encrypt: false,
            fail_decrypt: false,
        }
    }

    fn protected_crypto(kms: XorFixtureKms, principal_id: Option<&str>) -> ProtectedFileCrypto {
        ProtectedFileCrypto {
            provider: hosted_provider(kms),
            principal_id: principal_id.map(ToOwned::to_owned),
        }
    }

    fn store_context(store_id: &str) -> ProtectedStoreContext {
        let scope = MemoryKeyScope::new(
            &tenant("tandem-system"),
            DataClass::Restricted,
            Some(format!("tandem-governance-{store_id}")),
        )
        .with_org_unit(Some(store_id.to_string()));
        ProtectedStoreContext::new(
            store_id,
            ProtectedRecordContext::new(
                scope.clone(),
                format!("{store_id}:manifest"),
                format!("{store_id}:manifest-audit"),
            ),
            ProtectedRecordContext::new(
                scope,
                format!("{store_id}:head"),
                format!("{store_id}:head-audit"),
            ),
        )
    }

    fn json_records(entries: &[(&str, i64)]) -> Vec<ProtectedJsonRecord> {
        entries
            .iter()
            .map(|(key, value)| {
                ProtectedJsonRecord::new(
                    *key,
                    &serde_json::json!({"value": value}),
                    context("acme", "finance", key),
                )
                .expect("record")
            })
            .collect()
    }

    fn test_integrity_head_path(path: &Path) -> std::path::PathBuf {
        let file_name = path.file_name().expect("file name").to_string_lossy();
        path.with_file_name(format!("{file_name}.integrity"))
    }

    fn test_initialized_state_path(path: &Path) -> std::path::PathBuf {
        let file_name = path.file_name().expect("file name").to_string_lossy();
        path.with_file_name(format!("{file_name}.integrity.initialized"))
    }

    struct EnvRestore {
        provider: Option<String>,
        key_file: Option<String>,
        required: Option<String>,
        principal: Option<String>,
        kms_encrypt_command: Option<String>,
        kms_decrypt_command: Option<String>,
        kek_id: Option<String>,
        kek_version: Option<String>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                provider: std::env::var("TANDEM_MEMORY_DECRYPT_PROVIDER").ok(),
                key_file: std::env::var("TANDEM_MEMORY_LOCAL_KEY_FILE").ok(),
                required: std::env::var("TANDEM_MEMORY_ENCRYPTION_REQUIRED").ok(),
                principal: std::env::var(MEMORY_DECRYPT_PRINCIPAL_ENV).ok(),
                kms_encrypt_command: std::env::var("TANDEM_MEMORY_GOOGLE_KMS_ENCRYPT_COMMAND").ok(),
                kms_decrypt_command: std::env::var("TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND").ok(),
                kek_id: std::env::var("TANDEM_MEMORY_KEK_ID").ok(),
                kek_version: std::env::var("TANDEM_MEMORY_KEK_VERSION").ok(),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore_var("TANDEM_MEMORY_DECRYPT_PROVIDER", self.provider.as_deref());
            restore_var("TANDEM_MEMORY_LOCAL_KEY_FILE", self.key_file.as_deref());
            restore_var(
                "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
                self.required.as_deref(),
            );
            restore_var(MEMORY_DECRYPT_PRINCIPAL_ENV, self.principal.as_deref());
            restore_var(
                "TANDEM_MEMORY_GOOGLE_KMS_ENCRYPT_COMMAND",
                self.kms_encrypt_command.as_deref(),
            );
            restore_var(
                "TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND",
                self.kms_decrypt_command.as_deref(),
            );
            restore_var("TANDEM_MEMORY_KEK_ID", self.kek_id.as_deref());
            restore_var("TANDEM_MEMORY_KEK_VERSION", self.kek_version.as_deref());
        }
    }

    fn restore_var(key: &str, value: Option<&str>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn enable_local_encrypted(dir: &tempfile::TempDir) -> EnvRestore {
        let restore = EnvRestore::capture();
        std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "local-file");
        std::env::set_var(
            "TANDEM_MEMORY_LOCAL_KEY_FILE",
            dir.path().join("local_memory.key"),
        );
        std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
        std::env::remove_var(MEMORY_DECRYPT_PRINCIPAL_ENV);
        restore
    }

    #[tokio::test]
    #[serial]
    async fn legacy_local_key_json_round_trips_as_ciphertext() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _restore = enable_local_encrypted(&dir);
        let path = dir.path().join("policy_decisions.json");
        let record_context = context("acme", "finance", "decision-1");
        let payload = HashMap::from([(
            "decision-1".to_string(),
            serde_json::json!({"tenant": "acme", "secret": "finance-decision"}),
        )]);

        write_json_file(&path, &payload, &record_context)
            .await
            .expect("write encrypted");
        let raw = fs::read_to_string(&path).await.expect("read raw");
        assert!(is_encrypted_payload(&raw));
        assert!(!raw.contains("finance-decision"));

        let decoded: HashMap<String, serde_json::Value> = read_json_file(&path, &record_context)
            .await
            .expect("read encrypted");
        assert_eq!(decoded, payload);
    }

    #[test]
    #[serial]
    fn legacy_local_ciphertext_without_key_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let encrypted = {
            let _restore = enable_local_encrypted(&dir);
            encrypt_text(
                "protected-store-secret",
                &context("acme", "finance", "audit-1"),
            )
            .expect("encrypt with local provider")
        };
        let _restore = EnvRestore::capture();
        std::env::remove_var("TANDEM_MEMORY_DECRYPT_PROVIDER");
        std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");
        std::env::remove_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED");
        std::env::remove_var(MEMORY_DECRYPT_PRINCIPAL_ENV);

        let error = decrypt_text(&encrypted, &context("acme", "finance", "audit-1"))
            .expect_err("fail closed without key");
        assert!(
            error
                .to_string()
                .contains("requires a configured decrypt provider"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    #[serial]
    fn hosted_required_without_provisioned_kms_refuses_write() {
        let _restore = EnvRestore::capture();
        std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "true");
        std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "google_cloud_kms");
        std::env::set_var(MEMORY_DECRYPT_PRINCIPAL_ENV, RUNTIME_PRINCIPAL);
        std::env::remove_var("TANDEM_MEMORY_LOCAL_KEY_FILE");
        std::env::remove_var("TANDEM_MEMORY_GOOGLE_KMS_ENCRYPT_COMMAND");
        std::env::remove_var("TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND");
        std::env::remove_var("TANDEM_MEMORY_KEK_ID");
        std::env::remove_var("TANDEM_MEMORY_KEK_VERSION");

        let error = encrypt_text(
            "must not land as plaintext",
            &context("acme", "finance", "audit-1"),
        )
        .expect_err("fail closed");
        assert!(
            format!("{error:?}").contains("refusing to store plaintext"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    #[serial]
    fn hosted_record_round_trips_with_persisted_envelope() {
        let crypto = protected_crypto(healthy_kms(), Some(RUNTIME_PRINCIPAL));
        let context = context("acme", "finance", "audit-1");
        let stored = crypto
            .encrypt_record("finance audit secret", &context)
            .expect("encrypt");
        assert!(stored.starts_with(SCOPED_RECORD_PREFIX));
        assert!(!stored.contains("finance audit secret"));
        assert_eq!(
            crypto.decrypt_record(&stored, &context).expect("decrypt"),
            "finance audit secret"
        );
    }

    #[test]
    #[serial]
    fn hosted_record_denies_cross_tenant_and_cross_department_scope() {
        let crypto = protected_crypto(healthy_kms(), Some(RUNTIME_PRINCIPAL));
        let finance = context("acme", "finance", "audit-1");
        let stored = crypto
            .encrypt_record("finance audit secret", &finance)
            .expect("encrypt");

        for denied_context in [
            context("other", "finance", "audit-2"),
            context("acme", "engineering", "audit-3"),
        ] {
            let denied_stored = crypto
                .encrypt_record("other scoped secret", &denied_context)
                .expect("encrypt distinct scope");
            let original: ScopedEncryptedRecord =
                serde_json::from_str(stored.strip_prefix(SCOPED_RECORD_PREFIX).expect("prefix"))
                    .expect("record");
            let distinct: ScopedEncryptedRecord = serde_json::from_str(
                denied_stored
                    .strip_prefix(SCOPED_RECORD_PREFIX)
                    .expect("prefix"),
            )
            .expect("record");
            let original_envelope = original.envelope.expect("envelope");
            let distinct_envelope = distinct.envelope.expect("envelope");
            assert_ne!(original_envelope.key_scope, distinct_envelope.key_scope);
            assert_ne!(
                original_envelope.encryption_context_hash,
                distinct_envelope.encryption_context_hash
            );

            let err = crypto
                .decrypt_record(&stored, &denied_context)
                .expect_err("cross-scope decrypt must fail");
            assert!(
                format!("{err:?}").contains("trusted expected scope"),
                "{err:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn hosted_record_rejects_missing_or_corrupt_envelope_and_principal() {
        let crypto = protected_crypto(healthy_kms(), Some(RUNTIME_PRINCIPAL));
        let context = context("acme", "finance", "audit-1");
        let stored = crypto
            .encrypt_record("finance audit secret", &context)
            .expect("encrypt");
        let encoded = stored.strip_prefix(SCOPED_RECORD_PREFIX).expect("prefix");
        let mut record: ScopedEncryptedRecord = serde_json::from_str(encoded).expect("record");
        let mut scope_tampered = record.clone();
        scope_tampered
            .envelope
            .as_mut()
            .expect("envelope")
            .key_scope
            .org_unit = Some("engineering".to_string());
        let scope_tampered = format!(
            "{SCOPED_RECORD_PREFIX}{}",
            serde_json::to_string(&scope_tampered).expect("serialize")
        );
        let scope_error = crypto
            .decrypt_record(&scope_tampered, &context)
            .expect_err("scope-bound plaintext mismatch");
        assert!(
            format!("{scope_error:?}").contains("trusted expected scope"),
            "unexpected error: {scope_error:?}"
        );

        let mut corrupt = record.clone();
        corrupt
            .envelope
            .as_mut()
            .expect("envelope")
            .wrapped_dek
            .clear();
        let corrupt = format!(
            "{SCOPED_RECORD_PREFIX}{}",
            serde_json::to_string(&corrupt).expect("serialize")
        );
        let corrupt_error = crypto
            .decrypt_record(&corrupt, &context)
            .expect_err("corrupt envelope");
        assert!(
            format!("{corrupt_error:?}").contains("wrapped_dek"),
            "unexpected error: {corrupt_error:?}"
        );
        record.envelope = None;
        let missing = format!(
            "{SCOPED_RECORD_PREFIX}{}",
            serde_json::to_string(&record).expect("serialize")
        );
        let missing_error = crypto
            .decrypt_record(&missing, &context)
            .expect_err("missing envelope");
        assert!(
            format!("{missing_error:?}").contains("row envelope"),
            "unexpected error: {missing_error:?}"
        );
        assert!(crypto
            .decrypt_record("tgs1:{not-json", &context)
            .expect_err("corrupt envelope")
            .to_string()
            .contains("parse scoped"));
        let plaintext_error = crypto
            .decrypt_record("legacy plaintext", &context)
            .expect_err("hosted plaintext fallback");
        assert!(
            format!("{plaintext_error:?}").contains("legacy payload"),
            "unexpected error: {plaintext_error:?}"
        );

        let missing_principal = protected_crypto(healthy_kms(), None);
        assert!(missing_principal
            .decrypt_record(&stored, &context)
            .expect_err("missing principal")
            .to_string()
            .contains("runtime principal"));
    }

    #[test]
    #[serial]
    fn hosted_record_cryptographically_binds_policy_and_audit_anchors() {
        let crypto = protected_crypto(healthy_kms(), Some(RUNTIME_PRINCIPAL));
        let original_context = context("acme", "finance", "audit-1");
        let stored = crypto
            .encrypt_record("authority-bound governance record", &original_context)
            .expect("encrypt");
        let encoded = stored.strip_prefix(SCOPED_RECORD_PREFIX).expect("prefix");
        let original: ScopedEncryptedRecord = serde_json::from_str(encoded).expect("record");

        for (policy_decision_id, audit_id) in [
            ("governance-test-policy-attacker", "audit-1"),
            ("governance-test-policy", "audit-attacker"),
        ] {
            let mut edited = original.clone();
            let envelope = edited.envelope.as_mut().expect("envelope");
            envelope.policy_decision_id = policy_decision_id.to_string();
            envelope.audit_id = audit_id.to_string();
            envelope.encryption_context_hash =
                tandem_memory::envelope::memory_encryption_context_hash(
                    &envelope.key_scope,
                    &envelope.kek_id,
                    &envelope.kek_version,
                    &envelope.algorithm,
                    envelope.rotation_epoch,
                    &envelope.policy_decision_id,
                    &envelope.audit_id,
                )
                .expect("recompute edited metadata hash");
            let edited = format!(
                "{SCOPED_RECORD_PREFIX}{}",
                serde_json::to_string(&edited).expect("serialize")
            );
            let expected = ProtectedRecordContext::new(
                original_context.key_scope.clone(),
                policy_decision_id,
                audit_id,
            );
            let error = crypto
                .decrypt_record(&edited, &expected)
                .expect_err("KMS AAD must reject edited authority anchors");
            assert!(
                format!("{error:?}").contains("authenticated data mismatch"),
                "unexpected error: {error:?}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn hosted_collection_rejects_membership_order_and_full_rollback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("policy-decisions.json");
        let head_path = test_integrity_head_path(&path);
        let store = store_context("policy-decisions-test");
        let provider = hosted_provider(healthy_kms());

        with_test_crypto_provider(provider, Some(RUNTIME_PRINCIPAL), async {
            write_json_records_file(&path, &json_records(&[("a", 1), ("b", 2)]), &store)
                .await
                .expect("write generation one");
            let original_data = fs::read(&path).await.expect("data");
            let original_head = fs::read(&head_path).await.expect("head");
            read_text_file(&path, &store).await.expect("initial read");

            let stored = std::str::from_utf8(&original_data).expect("utf8");
            let encoded = stored
                .strip_prefix(AUTHENTICATED_COLLECTION_PREFIX)
                .expect("collection prefix");
            let manifest = crypto()
                .decrypt_record(encoded, &store.manifest)
                .expect("decrypt manifest");

            let mut deleted: serde_json::Value =
                serde_json::from_str(&manifest).expect("manifest json");
            deleted["records"]
                .as_array_mut()
                .expect("records")
                .remove(0);
            let deleted = crypto()
                .encrypt_record(
                    &serde_json::to_string(&deleted).expect("serialize"),
                    &store.manifest,
                )
                .expect("reseal deleted manifest");
            fs::write(&path, format!("{AUTHENTICATED_COLLECTION_PREFIX}{deleted}"))
                .await
                .expect("tamper deletion");
            assert!(read_text_file(&path, &store).await.is_err());

            let mut reordered: serde_json::Value =
                serde_json::from_str(&manifest).expect("manifest json");
            reordered["records"]
                .as_array_mut()
                .expect("records")
                .swap(0, 1);
            let reordered = crypto()
                .encrypt_record(
                    &serde_json::to_string(&reordered).expect("serialize"),
                    &store.manifest,
                )
                .expect("reseal reordered manifest");
            fs::write(
                &path,
                format!("{AUTHENTICATED_COLLECTION_PREFIX}{reordered}"),
            )
            .await
            .expect("tamper order");
            assert!(read_text_file(&path, &store).await.is_err());

            fs::write(&path, &original_data)
                .await
                .expect("restore data");
            write_json_records_file(&path, &json_records(&[("a", 3), ("b", 4)]), &store)
                .await
                .expect("write generation two");
            read_text_file(&path, &store)
                .await
                .expect("read generation two");

            fs::write(&path, &original_data)
                .await
                .expect("roll back data");
            fs::write(&head_path, &original_head)
                .await
                .expect("roll back head");
            let rollback_error = read_text_file(&path, &store)
                .await
                .expect_err("full rollback must fail in the running process");
            assert!(
                format!("{rollback_error:?}").contains("persistent initialized witness"),
                "unexpected error: {rollback_error:?}"
            );
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn hosted_collection_restart_fails_closed_after_data_or_head_deletion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("restart-deletion.json");
        let head_path = test_integrity_head_path(&path);
        let state_path = test_initialized_state_path(&path);
        let store = store_context("restart-deletion-test");

        with_test_crypto_provider(
            hosted_provider(healthy_kms()),
            Some(RUNTIME_PRINCIPAL),
            async {
                write_json_records_file(&path, &json_records(&[("a", 1)]), &store)
                    .await
                    .expect("initialize store");
                let committed_data = fs::read(&path).await.expect("data");
                let committed_head = fs::read(&head_path).await.expect("head");
                let committed_state = fs::read(&state_path).await.expect("initialized state");
                assert!(state_path.exists(), "initialized witness must be durable");

                integrity::forget_cached_head_for_test(&path).await;
                fs::remove_file(&path).await.expect("delete data");
                assert!(read_text_file(&path, &store).await.is_err());
                let data_error =
                    write_json_records_file(&path, &json_records(&[("replacement", 2)]), &store)
                        .await
                        .expect_err("initialized store must not be recreated after data deletion");
                assert!(
                    format!("{data_error:#}").contains("missing from an initialized store"),
                    "unexpected error: {data_error:?}"
                );

                fs::write(&path, &committed_data)
                    .await
                    .expect("restore data");
                integrity::forget_cached_head_for_test(&path).await;
                fs::remove_file(&head_path).await.expect("delete head");
                assert!(read_text_file(&path, &store).await.is_err());
                assert!(
                    write_json_records_file(&path, &json_records(&[("replacement", 3)]), &store)
                        .await
                        .is_err(),
                    "initialized store must not be recreated after head deletion"
                );

                fs::write(&head_path, &committed_head)
                    .await
                    .expect("restore head");
                fs::remove_file(&state_path)
                    .await
                    .expect("delete initialized state");
                integrity::forget_cached_head_for_test(&path).await;
                assert!(read_text_file(&path, &store).await.is_err());
                assert!(
                    write_json_records_file(&path, &json_records(&[("replacement", 4)]), &store)
                        .await
                        .is_err(),
                    "initialized store must reject witness deletion"
                );

                fs::write(&state_path, committed_state)
                    .await
                    .expect("restore initialized state");
                fs::remove_file(&path).await.expect("delete data again");
                fs::remove_file(&head_path)
                    .await
                    .expect("delete head again");
                integrity::forget_cached_head_for_test(&path).await;
                assert!(
                    write_json_records_file(&path, &json_records(&[("replacement", 5)]), &store)
                        .await
                        .is_err(),
                    "surviving initialized witness must reject coordinated data+head deletion"
                );
            },
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn hosted_collection_restart_rejects_stale_data_and_head_generation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("restart-stale.json");
        let head_path = test_integrity_head_path(&path);
        let store = store_context("restart-stale-test");

        with_test_crypto_provider(
            hosted_provider(healthy_kms()),
            Some(RUNTIME_PRINCIPAL),
            async {
                write_json_records_file(&path, &json_records(&[("a", 1)]), &store)
                    .await
                    .expect("write generation one");
                let stale_data = fs::read(&path).await.expect("generation-one data");
                let stale_head = fs::read(&head_path).await.expect("generation-one head");

                write_json_records_file(&path, &json_records(&[("a", 2)]), &store)
                    .await
                    .expect("write generation two");
                integrity::forget_cached_head_for_test(&path).await;
                fs::write(&path, stale_data).await.expect("roll back data");
                fs::write(&head_path, stale_head)
                    .await
                    .expect("roll back head");

                let error = read_text_file(&path, &store)
                    .await
                    .expect_err("persistent witness must reject stale generation");
                assert!(
                    format!("{error:#}").contains("persistent initialized witness"),
                    "unexpected error: {error:?}"
                );
            },
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn coordinated_witness_rollback_remains_a_local_residual_after_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("coordinated-residual.json");
        let head_path = test_integrity_head_path(&path);
        let state_path = test_initialized_state_path(&path);
        let store = store_context("coordinated-residual-test");

        with_test_crypto_provider(
            hosted_provider(healthy_kms()),
            Some(RUNTIME_PRINCIPAL),
            async {
                write_json_records_file(&path, &json_records(&[("a", 1)]), &store)
                    .await
                    .expect("write generation one");
                let stale_data = fs::read(&path).await.expect("generation-one data");
                let stale_head = fs::read(&head_path).await.expect("generation-one head");
                let stale_state = fs::read(&state_path).await.expect("generation-one state");

                write_json_records_file(&path, &json_records(&[("a", 2)]), &store)
                    .await
                    .expect("write generation two");
                fs::write(&path, stale_data).await.expect("roll back data");
                fs::write(&head_path, stale_head)
                    .await
                    .expect("roll back head");
                fs::write(&state_path, stale_state)
                    .await
                    .expect("roll back local witness");
                integrity::forget_cached_head_for_test(&path).await;

                let decoded = read_text_file(&path, &store)
                    .await
                    .expect("coordinated local rollback has no external monotonic root");
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&decoded).expect("json")["a"]
                        ["value"],
                    1
                );
            },
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn hosted_jsonl_rejects_deletion_reorder_and_replay() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("protected-audit.jsonl");
        let store = store_context("protected-audit-test");
        let provider = hosted_provider(healthy_kms());

        with_test_crypto_provider(provider, Some(RUNTIME_PRINCIPAL), async {
            for sequence in 1..=3 {
                append_jsonl_record_file(
                    &path,
                    &format!(r#"{{"seq":{sequence}}}"#),
                    &context("acme", "finance", &format!("audit-{sequence}")),
                    &store,
                    true,
                )
                .await
                .expect("append");
            }
            let original = fs::read_to_string(&path).await.expect("jsonl");
            let lines = original.lines().collect::<Vec<_>>();
            assert_eq!(
                read_jsonl_records_file(&path, &store)
                    .await
                    .expect("initial read")
                    .len(),
                3
            );

            fs::write(&path, format!("{}\n{}\n", lines[0], lines[2]))
                .await
                .expect("delete row");
            assert!(read_jsonl_records_file(&path, &store).await.is_err());

            fs::write(&path, format!("{}\n{}\n{}\n", lines[1], lines[0], lines[2]))
                .await
                .expect("reorder rows");
            assert!(read_jsonl_records_file(&path, &store).await.is_err());

            fs::write(
                &path,
                format!("{}\n{}\n{}\n{}\n", lines[0], lines[1], lines[2], lines[0]),
            )
            .await
            .expect("replay row");
            assert!(read_jsonl_records_file(&path, &store).await.is_err());

            fs::write(&path, original).await.expect("restore jsonl");
            assert_eq!(
                read_jsonl_records_file(&path, &store)
                    .await
                    .expect("restored read")
                    .len(),
                3
            );
        })
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn hosted_collection_kms_failure_preserves_committed_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("org-units.json");
        let head_path = test_integrity_head_path(&path);
        let store = store_context("org-units-test");

        with_test_crypto_provider(
            hosted_provider(healthy_kms()),
            Some(RUNTIME_PRINCIPAL),
            async {
                write_json_records_file(&path, &json_records(&[("unit-a", 1)]), &store)
                    .await
                    .expect("initial write");
            },
        )
        .await;
        let committed_data = fs::read(&path).await.expect("committed data");
        let committed_head = fs::read(&head_path).await.expect("committed head");

        let failing = XorFixtureKms {
            fail_encrypt: true,
            ..healthy_kms()
        };
        with_test_crypto_provider(hosted_provider(failing), Some(RUNTIME_PRINCIPAL), async {
            write_json_records_file(&path, &json_records(&[("unit-a", 2)]), &store)
                .await
                .expect_err("KMS write must fail");
        })
        .await;

        assert_eq!(
            fs::read(&path).await.expect("data after failure"),
            committed_data
        );
        assert_eq!(
            fs::read(&head_path).await.expect("head after failure"),
            committed_head
        );
    }

    #[test]
    #[serial]
    fn hosted_kms_unavailable_fails_write_and_cold_read() {
        let context = context("acme", "finance", "audit-1");
        let stored = protected_crypto(healthy_kms(), Some(RUNTIME_PRINCIPAL))
            .encrypt_record("finance audit secret", &context)
            .expect("encrypt");

        let failing_encrypt = XorFixtureKms {
            fail_encrypt: true,
            ..healthy_kms()
        };
        let unavailable_encrypt = protected_crypto(failing_encrypt, Some(RUNTIME_PRINCIPAL));
        let encrypt_error = unavailable_encrypt
            .encrypt_record("must fail", &context)
            .expect_err("KMS write failure");
        assert!(
            format!("{encrypt_error:?}").contains("fixture KMS encrypt unavailable"),
            "unexpected error: {encrypt_error:?}"
        );

        let failing_decrypt = XorFixtureKms {
            fail_decrypt: true,
            ..healthy_kms()
        };
        let unavailable_decrypt = protected_crypto(failing_decrypt, Some(RUNTIME_PRINCIPAL));
        let decrypt_error = unavailable_decrypt
            .decrypt_record(&stored, &context)
            .expect_err("KMS read failure");
        assert!(
            format!("{decrypt_error:?}").contains("fixture KMS decrypt unavailable"),
            "unexpected error: {decrypt_error:?}"
        );
    }
}
