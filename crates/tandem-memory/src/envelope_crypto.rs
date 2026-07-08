//! Hosted per-scope envelope encryption for memory fields (TAN-666).
//!
//! Wires together the four pieces that already existed but were never connected
//! to a running encrypt/decrypt path:
//!
//! * [`crate::envelope`] — the `MemoryKeyScope` (tenant × department × data_class
//!   × source) and the `MemoryEnvelopeMetadata` that travels with a sealed row;
//! * a KMS **wrap** provider ([`crate::kms_providers`]) that seals a fresh DEK on
//!   write, and a KMS **unwrap** provider that recovers it on read;
//! * the [`crate::decrypt_broker::MemoryDecryptBroker`], which authorizes each
//!   unwrap against the requesting principal (tenant, data class, source grant)
//!   and the envelope's key-lifecycle state; and
//! * the envelope-keyed [`crate::dek_cache::MemoryDekCache`], so a decrypt-heavy
//!   read path makes O(distinct envelope keys) KMS calls, not one per row.
//!
//! Sealing generates a fresh 32-byte DEK, AES-256-GCM-encrypts the field under
//! it, wraps the DEK with the scope's KEK (binding it to the scope via the
//! `encryption_context_hash` AAD), and emits a self-describing `tce1:` ciphertext
//! plus the envelope. Unsealing recovers the DEK — from cache, or via
//! broker-authorized KMS unwrap on a miss — and decrypts.
//!
//! **Layering note.** The DEK cache is a process-internal optimization: a hit
//! returns key material without re-consulting the broker, exactly like an OS page
//! cache holds already-decrypted bytes. Authorization of *which caller may read a
//! row's plaintext* is enforced upstream by the M1 access filter and, on a cache
//! miss, by the broker's principal check here. Cross-tenant confidentiality **at
//! rest** is guaranteed structurally: every scope gets a distinct KMS-wrapped
//! DEK, so a raw DB dump cannot decrypt one tenant's rows with another's DEK.

use crate::crypto::{decrypt_with_key, encrypt_with_key, random_dek, CIPHERTEXT_PREFIX, KEY_LEN};
use crate::decrypt_broker::{
    MemoryDecryptBroker, MemoryDecryptBrokerConfig, MemoryDecryptPrincipal, MemoryDecryptRequest,
    MemoryDekUnwrapProviderBox, MemoryDekWrapProviderBox, MemoryDekWrapRequest,
};
use crate::dek_cache::{MemoryDekCache, MemoryDekCacheKey};
use crate::envelope::{MemoryEnvelopeMetadata, MemoryKeyScope};
use crate::key_lifecycle::MemoryKeyLifecyclePolicy;
use crate::kms_providers::{
    GoogleCloudKmsExternalCommandClient, GoogleCloudKmsExternalEncryptCommandClient,
};
use crate::types::{MemoryError, MemoryResult};

use base64::Engine;
use sha2::{Digest, Sha256};

/// AES-256-GCM is the memory field cipher (matches [`crate::crypto`]).
const MEMORY_FIELD_ALGORITHM: &str = "AES-256-GCM";

const KEK_ID_ENV: &str = "TANDEM_MEMORY_KEK_ID";
const KEK_VERSION_ENV: &str = "TANDEM_MEMORY_KEK_VERSION";
const KEK_ROTATION_EPOCH_ENV: &str = "TANDEM_MEMORY_KEK_ROTATION_EPOCH";

/// A sealed memory field: the self-describing `tce1:` ciphertext plus the
/// envelope that must be stored (unencrypted) alongside it so the DEK can be
/// recovered on read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedMemoryField {
    pub ciphertext: String,
    pub envelope: MemoryEnvelopeMetadata,
}

/// Seals and unseals memory fields under per-scope, KMS-wrapped DEKs, caching the
/// unwrapped DEKs per envelope (keyed by scope + KEK version + rotation epoch +
/// wrapped-DEK fingerprint).
pub struct HostedMemoryEnvelopeCrypto {
    broker: MemoryDecryptBroker,
    wrap_provider: MemoryDekWrapProviderBox,
    unwrap_provider: MemoryDekUnwrapProviderBox,
    cache: MemoryDekCache,
    provider_id: String,
    runtime_principal_id: String,
    kek_id: String,
    kek_version: String,
    rotation_epoch: u64,
}

impl std::fmt::Debug for HostedMemoryEnvelopeCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedMemoryEnvelopeCrypto")
            .field("provider_id", &self.provider_id)
            .field("runtime_principal_id", &self.runtime_principal_id)
            .field("kek_id", &self.kek_id)
            .field("kek_version", &self.kek_version)
            .field("rotation_epoch", &self.rotation_epoch)
            .field("cache", &self.cache)
            .finish()
    }
}

impl HostedMemoryEnvelopeCrypto {
    /// Assemble a hosted crypto from pre-built parts (used by tests and callers
    /// that inject a KMS client directly).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        broker: MemoryDecryptBroker,
        wrap_provider: MemoryDekWrapProviderBox,
        unwrap_provider: MemoryDekUnwrapProviderBox,
        cache: MemoryDekCache,
        provider_id: impl Into<String>,
        runtime_principal_id: impl Into<String>,
        kek_id: impl Into<String>,
        kek_version: impl Into<String>,
        rotation_epoch: u64,
    ) -> Self {
        Self {
            broker,
            wrap_provider,
            unwrap_provider,
            cache,
            provider_id: provider_id.into(),
            runtime_principal_id: runtime_principal_id.into(),
            kek_id: kek_id.into(),
            kek_version: kek_version.into(),
            rotation_epoch,
        }
    }

    /// Build the hosted crypto from the environment. Returns `Ok(None)` — so the
    /// caller can fail closed to a pending/plaintext-rejecting mode — unless the
    /// runtime is fully provisioned: a hosted broker config, a KMS encrypt **and**
    /// decrypt command, and an explicit KEK id/version. This preserves today's
    /// fail-closed behavior when hosted KMS is requested but not yet wired.
    pub fn from_env() -> MemoryResult<Option<Self>> {
        let config = MemoryDecryptBrokerConfig::from_env()?;
        if !config.crypto_mode().is_hosted() {
            return Ok(None);
        }
        let encrypt_ready = GoogleCloudKmsExternalEncryptCommandClient::from_env()?.is_some();
        let decrypt_ready = GoogleCloudKmsExternalCommandClient::from_env()?.is_some();
        let kek_id = env_non_empty(KEK_ID_ENV);
        let kek_version = env_non_empty(KEK_VERSION_ENV);
        let (Some(kek_id), Some(kek_version)) = (kek_id, kek_version) else {
            return Ok(None);
        };
        if !encrypt_ready || !decrypt_ready {
            return Ok(None);
        }
        config.validate()?;
        let rotation_epoch = env_non_empty(KEK_ROTATION_EPOCH_ENV)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let wrap_provider = config.build_dek_wrap_provider()?.ok_or_else(|| {
            MemoryError::InvalidConfig("hosted memory encrypt provider unavailable".to_string())
        })?;
        let unwrap_provider = config.build_dek_unwrap_provider()?.ok_or_else(|| {
            MemoryError::InvalidConfig("hosted memory decrypt provider unavailable".to_string())
        })?;
        let provider_id = config.provider.clone();
        let runtime_principal_id = config.runtime_principal_id.clone();
        Ok(Some(Self::new(
            MemoryDecryptBroker::new(config)?,
            wrap_provider,
            unwrap_provider,
            MemoryDekCache::with_default_capacity(),
            provider_id,
            runtime_principal_id,
            kek_id,
            kek_version,
            rotation_epoch,
        )))
    }

    /// Seal a plaintext memory field under a fresh per-scope DEK. `policy_decision_id`
    /// and `audit_id` are the write-time authorization anchors stamped into the
    /// envelope; the broker later requires an unwrap to present the same ids.
    pub fn seal(
        &self,
        scope: &MemoryKeyScope,
        plaintext: &str,
        policy_decision_id: &str,
        audit_id: &str,
    ) -> MemoryResult<SealedMemoryField> {
        let (mut ciphertexts, envelope) =
            self.seal_fields(scope, &[plaintext], policy_decision_id, audit_id)?;
        Ok(SealedMemoryField {
            ciphertext: ciphertexts.remove(0),
            envelope,
        })
    }

    /// Seal several fields of one row (e.g. content + metadata) under a **single**
    /// fresh per-scope DEK and a **single** envelope, so the whole row costs one
    /// KMS wrap and shares one cache entry. Returns the ciphertexts in the input
    /// order plus the envelope to persist once alongside the row.
    pub fn seal_fields(
        &self,
        scope: &MemoryKeyScope,
        plaintexts: &[&str],
        policy_decision_id: &str,
        audit_id: &str,
    ) -> MemoryResult<(Vec<String>, MemoryEnvelopeMetadata)> {
        scope.validate_for_envelope()?;
        if policy_decision_id.trim().is_empty() || audit_id.trim().is_empty() {
            return Err(MemoryError::InvalidConfig(
                "sealing a memory field requires a policy decision id and audit id".to_string(),
            ));
        }
        let canonical_id = scope.canonical_id();
        let encryption_context_hash = self.encryption_context_hash(&canonical_id);
        let dek = random_dek()?;
        let wrapped = self.wrap_provider.wrap_dek(&MemoryDekWrapRequest {
            provider: self.provider_id.clone(),
            runtime_principal_id: self.runtime_principal_id.clone(),
            key_scope_id: canonical_id.clone(),
            kek_id: self.kek_id.clone(),
            kek_version: self.kek_version.clone(),
            plaintext_dek: dek.to_vec(),
            encryption_context_hash: encryption_context_hash.clone(),
            audit_id: audit_id.to_string(),
        })?;
        let wrapped_dek = base64::engine::general_purpose::STANDARD.encode(wrapped);
        let ciphertexts = plaintexts
            .iter()
            .map(|plaintext| encrypt_with_key(&dek, plaintext))
            .collect::<MemoryResult<Vec<_>>>()?;
        let envelope = MemoryEnvelopeMetadata {
            key_scope: scope.clone(),
            kek_id: self.kek_id.clone(),
            kek_version: self.kek_version.clone(),
            wrapped_dek,
            algorithm: MEMORY_FIELD_ALGORITHM.to_string(),
            encryption_context_hash,
            rotation_epoch: self.rotation_epoch,
            policy_decision_id: policy_decision_id.to_string(),
            audit_id: audit_id.to_string(),
        };
        // The writer is authorized by construction; caching the DEK now makes the
        // common seal→read-back path free of a KMS round-trip. The cache key
        // includes this row's own wrapped-DEK fingerprint so a later row in the
        // same scope (with its own fresh DEK) does not clobber this entry.
        self.cache.insert(
            MemoryDekCacheKey::new(
                canonical_id,
                self.kek_version.clone(),
                self.rotation_epoch,
                wrapped_dek_fingerprint(&envelope.wrapped_dek),
            ),
            dek,
        );
        Ok((ciphertexts, envelope))
    }

    /// Unseal a stored `tce1:` field. On a cache miss the unwrap is authorized by
    /// the broker against `principal` (tenant, data-class, source, key-lifecycle)
    /// and the envelope's own write-time ids, then the DEK is unwrapped via KMS
    /// and cached.
    pub fn unseal(
        &self,
        envelope: &MemoryEnvelopeMetadata,
        stored_ciphertext: &str,
        principal: &MemoryDecryptPrincipal,
        key_lifecycle_policy: Option<MemoryKeyLifecyclePolicy>,
    ) -> MemoryResult<String> {
        let hex_blob = stored_ciphertext
            .strip_prefix(CIPHERTEXT_PREFIX)
            .ok_or_else(|| {
                MemoryError::InvalidConfig(
                    "hosted memory mode requires encrypted rows (missing tce1 payload marker)"
                        .to_string(),
                )
            })?;
        let canonical_id = envelope.key_scope.canonical_id();
        let cache_key = MemoryDekCacheKey::new(
            canonical_id,
            envelope.kek_version.clone(),
            envelope.rotation_epoch,
            wrapped_dek_fingerprint(&envelope.wrapped_dek),
        );
        // Authorization runs on EVERY unseal, independent of the DEK cache. The
        // caller presents its own tenant scope; the broker denies unless that
        // scope owns the envelope (cross-tenant gate) and carries the required
        // data-class / source grants. The cache only ever saves the external KMS
        // unwrap — never the access-control decision — so a DEK cached at write
        // time cannot let an unauthorized reader bypass the broker.
        let request = MemoryDecryptRequest {
            envelope: envelope.clone(),
            tenant_scope: principal.tenant_scope.clone(),
            principal: principal.clone(),
            // The broker binds an unwrap to the envelope's own write-time
            // policy/audit ids, so they are taken from the envelope, not the
            // caller.
            policy_decision_id: envelope.policy_decision_id.clone(),
            audit_id: envelope.audit_id.clone(),
            break_glass_requested: false,
            key_lifecycle_policy,
        };
        let ticket = self.broker.authorize_unwrap(request)?.ok_or_else(|| {
            MemoryError::InvalidConfig(
                "hosted memory decrypt broker returned no unwrap ticket".to_string(),
            )
        })?;
        let dek = match self.cache.get(&cache_key) {
            Some(handle) => handle,
            None => {
                let dek_bytes = self.unwrap_provider.unwrap_dek(&ticket)?;
                let dek: [u8; KEY_LEN] = dek_bytes.as_slice().try_into().map_err(|_| {
                    MemoryError::InvalidConfig(format!(
                        "unwrapped memory DEK must be {KEY_LEN} bytes"
                    ))
                })?;
                self.cache.insert(cache_key, dek)
            }
        };
        decrypt_with_key(dek.expose(), hex_blob)
    }

    /// Unseal several fields of one row that were sealed together under one
    /// envelope. The first field authorizes + KMS-unwraps the shared DEK; the
    /// rest hit the cache, so a whole row costs one unwrap.
    pub fn unseal_fields(
        &self,
        envelope: &MemoryEnvelopeMetadata,
        stored_ciphertexts: &[&str],
        principal: &MemoryDecryptPrincipal,
        key_lifecycle_policy: Option<MemoryKeyLifecyclePolicy>,
    ) -> MemoryResult<Vec<String>> {
        stored_ciphertexts
            .iter()
            .map(|ciphertext| {
                self.unseal(
                    envelope,
                    ciphertext,
                    principal,
                    key_lifecycle_policy.clone(),
                )
            })
            .collect()
    }

    /// Drop every cached DEK for a scope (all key versions) on revocation.
    pub fn invalidate_scope(&self, canonical_id: &str) -> usize {
        self.cache.invalidate_canonical_id(canonical_id)
    }

    /// The shared DEK cache, for wiring one cache across multiple crypto handles.
    pub fn cache(&self) -> &MemoryDekCache {
        &self.cache
    }

    /// Deterministic context binding: ties an envelope to its scope + KEK so the
    /// wrapped DEK can only be unwrapped under the same context (used as KMS AAD).
    fn encryption_context_hash(&self, canonical_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"tandem-memory-envelope-v1\n");
        hasher.update(canonical_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.kek_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.kek_version.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.rotation_epoch.to_le_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(digest.len() * 2);
        for byte in digest {
            out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
            out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
        }
        out
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// A stable fingerprint of a row's wrapped DEK, used to give each envelope its
/// own DEK-cache entry. Two rows in the same scope carry different `wrapped_dek`s
/// (each seals a fresh DEK), so keying the cache on this prevents one row's DEK
/// from evicting/masking another's.
fn wrapped_dek_fingerprint(wrapped_dek: &str) -> String {
    let digest = Sha256::digest(wrapped_dek.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kms_providers::{
        GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
        GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
    };
    use crate::types::MemoryTenantScope;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tandem_enterprise_contract::DataClass;

    const RUNTIME_PRINCIPAL: &str = "runtime-memory-decryptor";
    const PROVIDER_ID: &str = "google_cloud_kms";
    const KEK_ID: &str = "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance";

    /// A reversible fixture KMS: wrapping and unwrapping a DEK are the same keyed
    /// XOR involution, so a DEK sealed under this KEK round-trips, and it asserts
    /// the scope-binding AAD is present on both sides. Counts unwrap calls so a
    /// test can prove the cache elides KMS round-trips.
    #[derive(Clone)]
    struct FixtureKms {
        fingerprint: u8,
        unwrap_calls: Arc<AtomicUsize>,
    }

    impl FixtureKms {
        fn new(fingerprint: u8) -> Self {
            Self {
                fingerprint,
                unwrap_calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl GoogleCloudKmsEncryptClient for FixtureKms {
        fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
            assert!(
                !request.additional_authenticated_data.is_empty(),
                "wrap must bind the scope context as AAD"
            );
            Ok(request
                .plaintext
                .iter()
                .map(|byte| byte ^ self.fingerprint)
                .collect())
        }
    }

    impl GoogleCloudKmsDecryptClient for FixtureKms {
        fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
            self.unwrap_calls.fetch_add(1, Ordering::SeqCst);
            assert!(
                !request.additional_authenticated_data.is_empty(),
                "unwrap must present the scope context as AAD"
            );
            Ok(request
                .ciphertext
                .iter()
                .map(|byte| byte ^ self.fingerprint)
                .collect())
        }
    }

    fn tenant(org: &str) -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: org.to_string(),
            workspace_id: "hq".to_string(),
            deployment_id: Some("prod".to_string()),
        }
    }

    fn finance_scope(org: &str) -> MemoryKeyScope {
        MemoryKeyScope::new(&tenant(org), DataClass::FinancialRecord, None)
            .with_org_unit(Some("department/finance".to_string()))
    }

    fn principal(org: &str, classes: Vec<DataClass>) -> MemoryDecryptPrincipal {
        MemoryDecryptPrincipal::retrieval_gateway(
            "kb-mcp-retrieval-gateway",
            tenant(org),
            classes,
            Vec::new(),
        )
    }

    fn hosted_with(kms: FixtureKms, cache: MemoryDekCache) -> HostedMemoryEnvelopeCrypto {
        let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL)
            .expect("hosted config");
        let broker = MemoryDecryptBroker::new(config).expect("broker");
        let wrap =
            GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL).expect("wrap");
        let unwrap = GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).expect("unwrap");
        HostedMemoryEnvelopeCrypto::new(
            broker,
            Box::new(wrap),
            Box::new(unwrap),
            cache,
            PROVIDER_ID,
            RUNTIME_PRINCIPAL,
            KEK_ID,
            "1",
            0,
        )
    }

    fn hosted() -> HostedMemoryEnvelopeCrypto {
        hosted_with(FixtureKms::new(0x5A), MemoryDekCache::new(64))
    }

    #[test]
    fn seal_produces_opaque_ciphertext_and_a_wrapped_dek() {
        let crypto = hosted();
        let sealed = crypto
            .seal(
                &finance_scope("acme"),
                "Invoice INV-2043: Hooli owes $120k",
                "decision-1",
                "audit-1",
            )
            .expect("seal");
        assert!(sealed.ciphertext.starts_with(CIPHERTEXT_PREFIX));
        assert!(!sealed.ciphertext.contains("120k"));
        assert!(!sealed.ciphertext.contains("Hooli"));
        assert_eq!(sealed.envelope.algorithm, MEMORY_FIELD_ALGORITHM);
        assert_eq!(sealed.envelope.kek_id, KEK_ID);
        assert!(!sealed.envelope.wrapped_dek.is_empty());
        assert!(!sealed.envelope.encryption_context_hash.is_empty());
    }

    #[test]
    fn seal_then_unseal_round_trips() {
        let crypto = hosted();
        let plaintext = "Hooli MSA auto-renews 2026-09-01 with a 14% uplift";
        let sealed = crypto
            .seal(&finance_scope("acme"), plaintext, "decision-1", "audit-1")
            .expect("seal");
        let recovered = crypto
            .unseal(
                &sealed.envelope,
                &sealed.ciphertext,
                &principal("acme", vec![DataClass::FinancialRecord]),
                None,
            )
            .expect("unseal");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn multiple_rows_in_one_scope_both_round_trip() {
        // Regression: each seal mints a fresh DEK, so two rows in the same scope +
        // KEK version carry different wrapped_deks. Both must remain readable —
        // the second seal must not evict or mask the first's cached DEK, and a
        // cold-cache read of either must recover its own DEK.
        let crypto = hosted();
        let scope = finance_scope("acme");
        let row_a = crypto
            .seal(&scope, "row A: invoice INV-1", "decision-1", "audit-1")
            .expect("seal A");
        let row_b = crypto
            .seal(&scope, "row B: invoice INV-2", "decision-1", "audit-1")
            .expect("seal B");
        assert_ne!(
            row_a.envelope.wrapped_dek, row_b.envelope.wrapped_dek,
            "each row seals its own DEK"
        );
        let who = principal("acme", vec![DataClass::FinancialRecord]);

        // Warm cache (both entries present after the two seals).
        assert_eq!(
            crypto
                .unseal(&row_a.envelope, &row_a.ciphertext, &who, None)
                .expect("A hot"),
            "row A: invoice INV-1"
        );
        assert_eq!(
            crypto
                .unseal(&row_b.envelope, &row_b.ciphertext, &who, None)
                .expect("B hot"),
            "row B: invoice INV-2"
        );

        // Cold cache: each row must unwrap its own DEK, in either order.
        crypto.cache().clear();
        assert_eq!(
            crypto
                .unseal(&row_b.envelope, &row_b.ciphertext, &who, None)
                .expect("B cold"),
            "row B: invoice INV-2"
        );
        assert_eq!(
            crypto
                .unseal(&row_a.envelope, &row_a.ciphertext, &who, None)
                .expect("A cold"),
            "row A: invoice INV-1"
        );
    }

    #[test]
    fn cache_elides_kms_unwrap_round_trips() {
        let kms = FixtureKms::new(0x5A);
        let unwrap_calls = Arc::clone(&kms.unwrap_calls);
        let crypto = hosted_with(kms, MemoryDekCache::new(64));
        let sealed = crypto
            .seal(&finance_scope("acme"), "cache me", "decision-1", "audit-1")
            .expect("seal");
        // Force the broker+KMS path by dropping the seal-time cache entry.
        crypto.cache().clear();

        let who = principal("acme", vec![DataClass::FinancialRecord]);
        for _ in 0..3 {
            assert_eq!(
                crypto
                    .unseal(&sealed.envelope, &sealed.ciphertext, &who, None)
                    .expect("unseal"),
                "cache me"
            );
        }
        assert_eq!(
            unwrap_calls.load(Ordering::SeqCst),
            1,
            "first unseal unwraps via KMS; the rest hit the cache"
        );
    }

    #[test]
    fn cross_tenant_principal_cannot_unseal() {
        let crypto = hosted();
        let sealed = crypto
            .seal(
                &finance_scope("acme"),
                "acme finance secret",
                "decision-1",
                "audit-1",
            )
            .expect("seal");
        crypto.cache().clear();
        // A principal scoped to a different tenant must be denied at the broker,
        // and a raw DB dump would carry acme's wrapped DEK — not the other
        // tenant's — so the row stays confidential across tenants.
        let err = crypto
            .unseal(
                &sealed.envelope,
                &sealed.ciphertext,
                &principal("hooli", vec![DataClass::FinancialRecord]),
                None,
            )
            .expect_err("cross-tenant unseal must be denied");
        assert!(err.to_string().contains("tenant scope"), "got: {err}");
    }

    #[test]
    fn principal_without_data_class_grant_is_denied() {
        let crypto = hosted();
        let sealed = crypto
            .seal(
                &finance_scope("acme"),
                "finance only",
                "decision-1",
                "audit-1",
            )
            .expect("seal");
        crypto.cache().clear();
        let err = crypto
            .unseal(
                &sealed.envelope,
                &sealed.ciphertext,
                &principal("acme", vec![DataClass::Internal]),
                None,
            )
            .expect_err("data-class denial");
        assert!(err.to_string().contains("data-class"), "got: {err}");
    }

    #[test]
    fn distinct_scopes_get_distinct_wrapped_deks() {
        let crypto = hosted();
        let acme = crypto
            .seal(&finance_scope("acme"), "same text", "decision-1", "audit-1")
            .expect("seal acme");
        let sales = crypto
            .seal(
                &MemoryKeyScope::new(&tenant("acme"), DataClass::CustomerData, None)
                    .with_org_unit(Some("department/sales".to_string())),
                "same text",
                "decision-1",
                "audit-1",
            )
            .expect("seal sales");
        assert_ne!(
            acme.envelope.wrapped_dek, sales.envelope.wrapped_dek,
            "different scopes must wrap different DEKs"
        );
        assert_ne!(
            acme.envelope.encryption_context_hash, sales.envelope.encryption_context_hash,
            "different scopes must bind different contexts"
        );
    }

    #[test]
    fn rotation_versions_of_a_scope_both_unseal() {
        // Two crypto handles share one cache but seal under different key versions
        // / rotation epochs (a rotation in flight). Both rows must unseal, and the
        // cache must hold both DEKs for the same scope simultaneously.
        let cache = MemoryDekCache::new(64);
        let v1 = hosted_with(FixtureKms::new(0x11), cache.clone());
        let mut v2 = hosted_with(FixtureKms::new(0x22), cache.clone());
        v2.kek_version = "2".to_string();
        v2.rotation_epoch = 1;

        let scope = finance_scope("acme");
        let sealed_v1 = v1
            .seal(&scope, "old-version row", "decision-1", "audit-1")
            .expect("seal v1");
        let sealed_v2 = v2
            .seal(&scope, "new-version row", "decision-1", "audit-1")
            .expect("seal v2");

        let who = principal("acme", vec![DataClass::FinancialRecord]);
        assert_eq!(
            v1.unseal(&sealed_v1.envelope, &sealed_v1.ciphertext, &who, None)
                .expect("unseal v1"),
            "old-version row"
        );
        assert_eq!(
            v2.unseal(&sealed_v2.envelope, &sealed_v2.ciphertext, &who, None)
                .expect("unseal v2"),
            "new-version row"
        );
        assert_eq!(cache.len(), 2, "both key versions coexist in the cache");
    }

    #[test]
    fn invalidate_scope_forces_a_fresh_unwrap() {
        let kms = FixtureKms::new(0x5A);
        let unwrap_calls = Arc::clone(&kms.unwrap_calls);
        let crypto = hosted_with(kms, MemoryDekCache::new(64));
        let scope = finance_scope("acme");
        let sealed = crypto
            .seal(&scope, "revoke me", "decision-1", "audit-1")
            .expect("seal");
        let who = principal("acme", vec![DataClass::FinancialRecord]);

        // Seal cached the DEK, so this unseal is a hit (no KMS call).
        crypto
            .unseal(&sealed.envelope, &sealed.ciphertext, &who, None)
            .expect("hit");
        assert_eq!(unwrap_calls.load(Ordering::SeqCst), 0);

        // Revoke the scope's cached DEK; the next unseal must go back to KMS.
        let dropped = crypto.invalidate_scope(&scope.canonical_id());
        assert_eq!(dropped, 1);
        crypto
            .unseal(&sealed.envelope, &sealed.ciphertext, &who, None)
            .expect("miss");
        assert_eq!(unwrap_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unseal_rejects_plaintext_rows() {
        let crypto = hosted();
        let sealed = crypto
            .seal(&finance_scope("acme"), "sealed", "decision-1", "audit-1")
            .expect("seal");
        let err = crypto
            .unseal(
                &sealed.envelope,
                "legacy plaintext row",
                &principal("acme", vec![DataClass::FinancialRecord]),
                None,
            )
            .expect_err("plaintext must be rejected in hosted mode");
        assert!(err.to_string().contains("tce1"), "got: {err}");
    }

    #[test]
    fn seal_rejects_wildcard_scope() {
        let crypto = hosted();
        let mut scope = finance_scope("acme");
        scope.org_unit = Some("*".to_string());
        assert!(crypto.seal(&scope, "x", "decision-1", "audit-1").is_err());
    }
}
