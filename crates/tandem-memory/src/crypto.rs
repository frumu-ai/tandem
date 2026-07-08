//! Memory payload encryption (ciphertext-at-rest).
//!
//! Semantic memory text columns are encrypted before they are written to the
//! database and decrypted on read, so a raw database dump does not reveal tenant
//! memory plaintext. This is driven by the crypto mode resolved in
//! [`crate::decrypt_broker`]:
//!
//! - **Local plaintext** (default single-user): a no-op — existing/local data is
//!   stored and read as plaintext, relying on host/file security.
//! - **Local encrypted**: AES-256-GCM with a host key file (0600) under the
//!   tandem home directory, generated on first use.
//! - **Hosted KMS**: requires a KMS-backed DEK via the decrypt broker. Until a
//!   KMS provider is provisioned, hosted mode **fails closed** on write rather
//!   than silently storing plaintext.
//!
//! Stored ciphertext is self-describing (`tce1:<hex(nonce||ciphertext+tag)>`).
//! In local plaintext and local-encrypted modes, legacy plaintext rows are read
//! as plain text for compatibility, but hosted modes reject plaintext rows to
//! enforce fail-closed behavior at rest.
//!
//! Embeddings (sqlite-vec KNN) and the FTS-indexed `memory_records.content`
//! column cannot be encrypted without breaking similarity/full-text search; they
//! are classified as search-required plaintext and governed by authority-scoped
//! reads instead. See `docs/internal` / the BR-14 notes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};

use crate::decrypt_broker::{MemoryCryptoMode, MemoryDecryptBrokerConfig, MemoryDecryptPrincipal};
use crate::envelope::{MemoryEnvelopeMetadata, MemoryKeyScope};
use crate::envelope_crypto::HostedMemoryEnvelopeCrypto;
use crate::key_lifecycle::MemoryKeyLifecyclePolicy;
use crate::types::{MemoryError, MemoryResult};

/// Self-describing prefix for an encrypted memory field (tandem crypto
/// envelope, version 1).
pub(crate) const CIPHERTEXT_PREFIX: &str = "tce1:";
const LOCAL_KEY_FILE_ENV: &str = "TANDEM_MEMORY_LOCAL_KEY_FILE";
const NONCE_LEN: usize = 12;
pub(crate) const KEY_LEN: usize = 32;

#[derive(Clone)]
enum CryptoInner {
    /// No encryption (local plaintext / backward compatibility). This is the
    /// default single-tenant mode — no enterprise, no KMS, no broker involved.
    Plaintext,
    /// Local AES-256-GCM with a single host-held key. Single-tenant encrypted
    /// mode; still no KMS/enterprise dependency, and the key scope is ignored.
    LocalKey([u8; KEY_LEN]),
    /// Hosted, multi-tenant mode: per-scope DEKs are wrapped by an external KMS
    /// and cached (TAN-666). Only ever constructed when a hosted deployment is
    /// fully provisioned (KMS commands + KEK); single-tenant instances never
    /// reach this variant.
    Hosted(Arc<HostedMemoryEnvelopeCrypto>),
    /// Hosted mode requested but its KMS-backed DEK provider is not yet available;
    /// writes fail closed so plaintext is never persisted under a hosted
    /// requirement.
    HostedPending,
}

/// Encrypts/decrypts individual memory text fields according to the active
/// crypto mode. Cheap to clone.
#[derive(Clone)]
pub struct MemoryCryptoProvider {
    inner: CryptoInner,
}

impl std::fmt::Debug for MemoryCryptoProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.inner {
            CryptoInner::Plaintext => "plaintext",
            CryptoInner::LocalKey(_) => "local_key",
            CryptoInner::Hosted(_) => "hosted_kms",
            CryptoInner::HostedPending => "hosted_pending",
        };
        f.debug_struct("MemoryCryptoProvider")
            .field("mode", &label)
            .finish()
    }
}

impl MemoryCryptoProvider {
    /// A no-op provider: fields are stored and read as plaintext.
    pub fn plaintext() -> Self {
        Self {
            inner: CryptoInner::Plaintext,
        }
    }

    /// A local AES-256-GCM provider backed by the given 256-bit key.
    pub fn local_key(key: [u8; KEY_LEN]) -> Self {
        Self {
            inner: CryptoInner::LocalKey(key),
        }
    }

    /// Resolve the provider from the environment-selected crypto mode.
    pub fn from_env() -> Self {
        let config = MemoryDecryptBrokerConfig::from_env()
            .unwrap_or_else(|_| MemoryDecryptBrokerConfig::local_disabled());
        Self::from_mode(config.crypto_mode())
    }

    /// Build a provider for an explicit crypto mode.
    pub fn from_mode(mode: MemoryCryptoMode) -> Self {
        match mode {
            MemoryCryptoMode::LocalPlaintext => Self::plaintext(),
            MemoryCryptoMode::LocalEncrypted { .. } => {
                match load_or_create_local_key(&local_key_path()) {
                    Ok(key) => Self::local_key(key),
                    Err(err) => {
                        tracing::error!(
                        "local memory encryption is configured but the key could not be loaded ({err}); failing closed"
                    );
                        Self {
                            inner: CryptoInner::HostedPending,
                        }
                    }
                }
            }
            // Hosted KMS-backed encryption (BR-12, TAN-666). Wire the real
            // per-scope envelope crypto when the deployment is fully provisioned
            // (hosted broker config + KMS encrypt/decrypt commands + KEK);
            // otherwise fail closed rather than store plaintext. Single-tenant
            // instances never select this mode, so they never require a KMS.
            MemoryCryptoMode::HostedKms { .. } => match HostedMemoryEnvelopeCrypto::from_env() {
                Ok(Some(hosted)) => Self {
                    inner: CryptoInner::Hosted(Arc::new(hosted)),
                },
                Ok(None) => {
                    tracing::warn!(
                        "hosted memory encryption is required but the KMS provider/KEK is not fully provisioned; failing closed (memory writes will be rejected)"
                    );
                    Self {
                        inner: CryptoInner::HostedPending,
                    }
                }
                Err(err) => {
                    tracing::error!(
                        "hosted memory encryption could not be initialized ({err}); failing closed"
                    );
                    Self {
                        inner: CryptoInner::HostedPending,
                    }
                }
            },
        }
    }

    /// True when fields are stored as plaintext (no encryption applied).
    pub fn is_plaintext(&self) -> bool {
        matches!(self.inner, CryptoInner::Plaintext)
    }

    /// True when this provider seals per-scope envelopes (hosted KMS mode) and so
    /// requires the scope-aware [`encrypt_field_scoped`](Self::encrypt_field_scoped)
    /// / [`decrypt_field_scoped`](Self::decrypt_field_scoped) API.
    pub fn is_hosted(&self) -> bool {
        matches!(self.inner, CryptoInner::Hosted(_))
    }

    /// Encrypt a memory text field for storage. Plaintext mode returns the input
    /// unchanged; hosted modes fail closed because sealing requires a key scope
    /// (use [`encrypt_field_scoped`](Self::encrypt_field_scoped)).
    pub fn encrypt_field(&self, plaintext: &str) -> MemoryResult<String> {
        match &self.inner {
            CryptoInner::Plaintext => Ok(plaintext.to_string()),
            CryptoInner::LocalKey(key) => encrypt_with_key(key, plaintext),
            CryptoInner::Hosted(_) => Err(MemoryError::InvalidConfig(
                "hosted memory encryption requires a key scope; use encrypt_field_scoped (fail-closed)"
                    .to_string(),
            )),
            CryptoInner::HostedPending => Err(MemoryError::InvalidConfig(
                "hosted memory encryption requires a provisioned KMS provider; refusing to store plaintext (fail-closed)"
                    .to_string(),
            )),
        }
    }

    /// Encrypt a memory field, honoring the per-scope envelope in hosted mode.
    ///
    /// Returns the stored ciphertext and, in hosted mode, the
    /// [`MemoryEnvelopeMetadata`] that must be persisted (unencrypted) alongside
    /// the row so the DEK can be recovered on read. Local/plaintext modes ignore
    /// the scope and return `None` for the envelope — single-tenant behavior is
    /// unchanged.
    pub fn encrypt_field_scoped(
        &self,
        plaintext: &str,
        scope: &MemoryKeyScope,
        policy_decision_id: &str,
        audit_id: &str,
    ) -> MemoryResult<(String, Option<MemoryEnvelopeMetadata>)> {
        match &self.inner {
            CryptoInner::Plaintext => Ok((plaintext.to_string(), None)),
            CryptoInner::LocalKey(key) => Ok((encrypt_with_key(key, plaintext)?, None)),
            CryptoInner::Hosted(hosted) => {
                let sealed = hosted.seal(scope, plaintext, policy_decision_id, audit_id)?;
                Ok((sealed.ciphertext, Some(sealed.envelope)))
            }
            CryptoInner::HostedPending => Err(MemoryError::InvalidConfig(
                "hosted memory encryption requires a provisioned KMS provider; refusing to store plaintext (fail-closed)"
                    .to_string(),
            )),
        }
    }

    /// Decrypt a stored memory text field.
    ///
    /// - In plaintext and local-encrypted modes, values without the encryption
    ///   prefix are treated as legacy plaintext for compatibility.
    /// - In hosted mode, plaintext rows are rejected to avoid leaving memory
    ///   readable at rest under encryption-required semantics.
    pub fn decrypt_field(&self, stored: &str) -> MemoryResult<String> {
        let Some(hex_blob) = stored.strip_prefix(CIPHERTEXT_PREFIX) else {
            return match &self.inner {
                CryptoInner::Plaintext | CryptoInner::LocalKey(_) => Ok(stored.to_string()),
                CryptoInner::Hosted(_) => Err(MemoryError::InvalidConfig(
                    "hosted memory mode requires encrypted rows (missing tce1 payload marker)"
                        .to_string(),
                )),
                CryptoInner::HostedPending => Err(MemoryError::InvalidConfig(
                    "hosted memory mode requires encrypted rows (missing tce1 payload marker)"
                        .to_string(),
                )),
            };
        };

        match &self.inner {
            CryptoInner::LocalKey(key) => decrypt_with_key(key, hex_blob),
            CryptoInner::Plaintext => Ok(stored.to_string()),
            CryptoInner::Hosted(_) => Err(MemoryError::InvalidConfig(
                "hosted memory decryption requires the row envelope; use decrypt_field_scoped"
                    .to_string(),
            )),
            CryptoInner::HostedPending => Err(MemoryError::InvalidConfig(
                "encrypted memory field cannot be read without the configured decryption key"
                    .to_string(),
            )),
        }
    }

    /// Decrypt a memory field, honoring the per-scope envelope in hosted mode.
    ///
    /// Local/plaintext modes ignore `envelope`/`principal` and behave exactly like
    /// [`decrypt_field`](Self::decrypt_field) — single-tenant reads are unchanged.
    /// Hosted mode requires the row's envelope and a decrypt principal; the DEK is
    /// served from cache or unwrapped via the broker-authorized KMS path.
    pub fn decrypt_field_scoped(
        &self,
        stored: &str,
        envelope: Option<&MemoryEnvelopeMetadata>,
        principal: Option<&MemoryDecryptPrincipal>,
        key_lifecycle_policy: Option<MemoryKeyLifecyclePolicy>,
    ) -> MemoryResult<String> {
        match &self.inner {
            CryptoInner::Plaintext | CryptoInner::LocalKey(_) => self.decrypt_field(stored),
            CryptoInner::Hosted(hosted) => {
                let envelope = envelope.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "hosted memory decryption requires the row envelope".to_string(),
                    )
                })?;
                let principal = principal.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "hosted memory decryption requires a decrypt principal".to_string(),
                    )
                })?;
                hosted.unseal(envelope, stored, principal, key_lifecycle_policy)
            }
            CryptoInner::HostedPending => self.decrypt_field(stored),
        }
    }

    /// Encrypt a whole row's fields (e.g. `[content, metadata]`) under one key
    /// scope. In hosted mode every field shares a single DEK and envelope, so the
    /// row costs one KMS wrap; the returned envelope must be persisted **once**
    /// alongside the row (the `crypto_envelope` column) and is `None` in
    /// local/plaintext modes, which encrypt each field with the single host key.
    pub fn encrypt_row_scoped(
        &self,
        plaintexts: &[&str],
        scope: &MemoryKeyScope,
        policy_decision_id: &str,
        audit_id: &str,
    ) -> MemoryResult<(Vec<String>, Option<MemoryEnvelopeMetadata>)> {
        match &self.inner {
            CryptoInner::Plaintext => {
                Ok((plaintexts.iter().map(|p| p.to_string()).collect(), None))
            }
            CryptoInner::LocalKey(key) => {
                let ciphertexts = plaintexts
                    .iter()
                    .map(|plaintext| encrypt_with_key(key, plaintext))
                    .collect::<MemoryResult<Vec<_>>>()?;
                Ok((ciphertexts, None))
            }
            CryptoInner::Hosted(hosted) => {
                let (ciphertexts, envelope) =
                    hosted.seal_fields(scope, plaintexts, policy_decision_id, audit_id)?;
                Ok((ciphertexts, Some(envelope)))
            }
            CryptoInner::HostedPending => Err(MemoryError::InvalidConfig(
                "hosted memory encryption requires a provisioned KMS provider; refusing to store plaintext (fail-closed)"
                    .to_string(),
            )),
        }
    }

    /// Decrypt a whole row's fields. The caller passes the row's stored
    /// `crypto_envelope` (if any): `Some` means the row was hosted-sealed and is
    /// decrypted via the broker-authorized KMS path (a `principal` is required);
    /// `None` means a legacy/local row decrypted with the single host key. This
    /// makes reads branch on the row, not the process mode, so hosted-sealed and
    /// legacy rows coexist. Fail-closed: a hosted-sealed row read by a local
    /// provider, or without a principal, is rejected rather than leaked.
    pub fn decrypt_row_scoped(
        &self,
        stored: &[&str],
        envelope: Option<&MemoryEnvelopeMetadata>,
        principal: Option<&MemoryDecryptPrincipal>,
        key_lifecycle_policy: Option<MemoryKeyLifecyclePolicy>,
    ) -> MemoryResult<Vec<String>> {
        match (&self.inner, envelope) {
            (CryptoInner::Hosted(hosted), Some(envelope)) => {
                let principal = principal.ok_or_else(|| {
                    MemoryError::InvalidConfig(
                        "hosted memory decryption requires a decrypt principal".to_string(),
                    )
                })?;
                hosted.unseal_fields(envelope, stored, principal, key_lifecycle_policy)
            }
            // A hosted-sealed row (has an envelope) read by a non-hosted provider
            // cannot be decrypted here — fail closed rather than return ciphertext.
            (_, Some(_)) => Err(MemoryError::InvalidConfig(
                "row is hosted-sealed (carries a crypto envelope) but the memory crypto provider is not hosted-KMS".to_string(),
            )),
            // No envelope: a legacy / local-key / plaintext row.
            (_, None) => stored
                .iter()
                .map(|value| self.decrypt_field(value))
                .collect(),
        }
    }

    /// Encrypt an optional JSON-ish metadata string if present.
    pub fn encrypt_optional(&self, value: Option<&str>) -> MemoryResult<Option<String>> {
        match value {
            Some(text) => Ok(Some(self.encrypt_field(text)?)),
            None => Ok(None),
        }
    }

    /// Decrypt an optional stored field if present.
    pub fn decrypt_optional(&self, value: Option<&str>) -> MemoryResult<Option<String>> {
        match value {
            Some(text) => Ok(Some(self.decrypt_field(text)?)),
            None => Ok(None),
        }
    }
}

impl Default for MemoryCryptoProvider {
    fn default() -> Self {
        Self::plaintext()
    }
}

/// Generate a fresh random 256-bit data-encryption key.
pub(crate) fn random_dek() -> MemoryResult<[u8; KEY_LEN]> {
    random_bytes::<KEY_LEN>()
}

pub(crate) fn encrypt_with_key(key: &[u8; KEY_LEN], plaintext: &str) -> MemoryResult<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce_bytes = random_bytes::<NONCE_LEN>()?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|_| MemoryError::InvalidConfig("memory field encryption failed".to_string()))?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(format!("{CIPHERTEXT_PREFIX}{}", to_hex(&blob)))
}

pub(crate) fn decrypt_with_key(key: &[u8; KEY_LEN], hex_blob: &str) -> MemoryResult<String> {
    let blob = from_hex(hex_blob).ok_or_else(|| {
        MemoryError::InvalidConfig("memory field ciphertext is malformed".to_string())
    })?;
    if blob.len() < NONCE_LEN {
        return Err(MemoryError::InvalidConfig(
            "memory field ciphertext is too short".to_string(),
        ));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| MemoryError::InvalidConfig("memory field decryption failed".to_string()))?;
    String::from_utf8(plaintext).map_err(|_| {
        MemoryError::InvalidConfig("decrypted memory field is not valid UTF-8".to_string())
    })
}

fn random_bytes<const N: usize>() -> MemoryResult<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf)
        .map_err(|err| MemoryError::InvalidConfig(format!("secure RNG unavailable: {err}")))?;
    Ok(buf)
}

fn local_key_path() -> PathBuf {
    if let Ok(explicit) = std::env::var(LOCAL_KEY_FILE_ENV) {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".tandem").join("memory").join("local_memory.key")
}

/// Load a 256-bit local key from `path`, generating and persisting one (0600) on
/// first use.
fn load_or_create_local_key(path: &Path) -> MemoryResult<[u8; KEY_LEN]> {
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() == KEY_LEN {
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        // Tolerate a hex-encoded key file.
        if let Some(decoded) = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| from_hex(text.trim()))
        {
            if decoded.len() == KEY_LEN {
                let mut key = [0u8; KEY_LEN];
                key.copy_from_slice(&decoded);
                return Ok(key);
            }
        }
        return Err(MemoryError::InvalidConfig(format!(
            "local memory key file `{}` is not a valid 256-bit key",
            path.display()
        )));
    }

    let key = random_bytes::<KEY_LEN>()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            MemoryError::InvalidConfig(format!("failed to create local key directory: {err}"))
        })?;
    }
    std::fs::write(path, key).map_err(|err| {
        MemoryError::InvalidConfig(format!("failed to write local memory key file: {err}"))
    })?;
    set_key_file_permissions(path);
    Ok(key)
}

#[cfg(unix)]
fn set_key_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_key_file_permissions(_path: &Path) {}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_provider_is_noop_and_passes_through_legacy() {
        let provider = MemoryCryptoProvider::plaintext();
        assert!(provider.is_plaintext());
        assert_eq!(
            provider.encrypt_field("secret memory").unwrap(),
            "secret memory"
        );
        assert_eq!(
            provider.decrypt_field("secret memory").unwrap(),
            "secret memory"
        );
    }

    #[test]
    fn local_key_round_trips_and_is_ciphertext_at_rest() {
        let provider = MemoryCryptoProvider::local_key([7u8; KEY_LEN]);
        let plaintext = "tenant A confidential note: launch date is 2026-09-01";
        let stored = provider.encrypt_field(plaintext).unwrap();

        // Stored form is opaque ciphertext, not the plaintext.
        assert!(stored.starts_with(CIPHERTEXT_PREFIX));
        assert!(!stored.contains("confidential"));
        assert!(!stored.contains("launch date"));

        // Round-trips back to plaintext.
        assert_eq!(provider.decrypt_field(&stored).unwrap(), plaintext);
    }

    #[test]
    fn encryption_uses_a_fresh_nonce_each_time() {
        let provider = MemoryCryptoProvider::local_key([3u8; KEY_LEN]);
        let a = provider.encrypt_field("same plaintext").unwrap();
        let b = provider.encrypt_field("same plaintext").unwrap();
        assert_ne!(
            a, b,
            "nonce reuse would make identical plaintext produce identical ciphertext"
        );
        assert_eq!(provider.decrypt_field(&a).unwrap(), "same plaintext");
        assert_eq!(provider.decrypt_field(&b).unwrap(), "same plaintext");
    }

    #[test]
    fn local_key_reads_legacy_plaintext_rows() {
        // Existing plaintext data (no prefix) remains readable after enabling
        // local encryption — no migration required.
        let provider = MemoryCryptoProvider::local_key([9u8; KEY_LEN]);
        assert_eq!(
            provider.decrypt_field("legacy plaintext").unwrap(),
            "legacy plaintext"
        );
    }

    #[test]
    fn wrong_key_cannot_decrypt() {
        let writer = MemoryCryptoProvider::local_key([1u8; KEY_LEN]);
        let reader = MemoryCryptoProvider::local_key([2u8; KEY_LEN]);
        let stored = writer.encrypt_field("cross-tenant secret").unwrap();
        assert!(reader.decrypt_field(&stored).is_err());
    }

    #[test]
    fn hosted_pending_fails_closed_on_write() {
        let provider = MemoryCryptoProvider::from_mode(MemoryCryptoMode::HostedKms {
            provider: "google_cloud_kms".to_string(),
        });
        assert!(
            provider
                .encrypt_field("must not be stored as plaintext")
                .is_err(),
            "hosted mode without a KMS provider must fail closed"
        );
        // Plaintext mode reading an encrypted value also fails closed.
        assert!(provider
            .decrypt_field(&format!("{CIPHERTEXT_PREFIX}deadbeef"))
            .is_err());

        assert!(
            provider.decrypt_field("legacy memory row").is_err(),
            "hosted mode should reject plaintext rows to avoid compatibility leakage"
        );
    }

    #[test]
    fn local_encrypted_mode_generates_and_reuses_a_key_file() {
        let dir = std::env::temp_dir().join(format!("tandem-mem-key-{}", uuid::Uuid::new_v4()));
        let key_path = dir.join("local_memory.key");
        let key1 = load_or_create_local_key(&key_path).expect("create key");
        assert!(key_path.exists());
        let key2 = load_or_create_local_key(&key_path).expect("reload key");
        assert_eq!(key1, key2, "key file must be stable across loads");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "key file must be 0600");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_round_trips() {
        let bytes = [0u8, 1, 15, 16, 255, 128, 64];
        let hex = to_hex(&bytes);
        assert_eq!(from_hex(&hex).unwrap(), bytes);
        assert!(from_hex("xyz").is_none());
        assert!(from_hex("abc").is_none()); // odd length
    }
}
