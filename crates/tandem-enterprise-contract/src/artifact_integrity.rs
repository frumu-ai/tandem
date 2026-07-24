// Copyright (c) 2026 Frumu LTD
// Licensed under the MIT OR Apache-2.0 license.

use std::collections::HashSet;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ARTIFACT_MANIFEST_SCHEMA: &str = "tandem-artifact-manifest/v1";
pub const ARTIFACT_MANIFEST_FILENAME: &str = "tandem-artifacts-v1.json";
pub const ARTIFACT_MANIFEST_SIGNATURE_FILENAME: &str = "tandem-artifacts-v1.json.minisig";
pub const MAX_ARTIFACT_MANIFEST_BYTES: usize = 512 * 1024;
pub const MAX_ARTIFACT_SIGNATURE_BYTES: usize = 16 * 1024;
pub const MAX_SIGNED_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 64;
const MAX_SBOM_BYTES: u64 = 16 * 1024 * 1024;

// This is the public half of the same Minisign key already pinned by the
// desktop Tauri updater. Release workflows sign the runtime-artifact manifest
// with the corresponding repository secret; consumers never accept an
// unsigned fallback.
const TAURI_UPDATER_MINISIGN_PUBLIC_KEY: &str =
    "RWTS6MAKVnCuE0xUcsg7GkW34AGdf1Qal7NxCNkxqM+ZO0ZuMIIwqfeO";

pub const EMBEDDED_ARTIFACT_TRUST_KEYS: &[ArtifactTrustKey<'static>] = &[ArtifactTrustKey {
    key_id: "tauri-updater-2026",
    public_key_base64: TAURI_UPDATER_MINISIGN_PUBLIC_KEY,
    status: ArtifactTrustKeyStatus::Active,
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactTrustKeyStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactTrustKey<'a> {
    pub key_id: &'a str,
    pub public_key_base64: &'a str,
    pub status: ArtifactTrustKeyStatus,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactManifestExpectation<'a> {
    pub source_repository: &'a str,
    pub release: &'a str,
    pub version: &'a str,
    pub allowed_workflows: &'a [&'a str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    pub schema: String,
    pub release: String,
    pub version: String,
    pub generated_at: String,
    pub source_repository: String,
    pub source_commit: String,
    pub artifacts: Vec<ArtifactManifestEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Browser,
    Engine,
    EngineEnterprise,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPlatform {
    Darwin,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactArchitecture {
    Arm64,
    X64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifestEntry {
    pub kind: ArtifactKind,
    pub version: String,
    pub platform: ArtifactPlatform,
    pub architecture: ArtifactArchitecture,
    pub filename: String,
    pub length: u64,
    pub sha256: String,
    pub sbom: ArtifactSbom,
    pub provenance: ArtifactProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSbom {
    pub filename: String,
    pub length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactProvenance {
    pub source_repository: String,
    pub source_commit: String,
    pub workflow: String,
    pub run_id: u64,
    pub builder_id: String,
}

#[derive(Debug, Clone)]
pub struct VerifiedArtifactManifest {
    manifest: ArtifactManifest,
    signing_key_id: String,
}

impl VerifiedArtifactManifest {
    pub fn manifest(&self) -> &ArtifactManifest {
        &self.manifest
    }

    pub fn signing_key_id(&self) -> &str {
        &self.signing_key_id
    }

    pub fn artifact(
        &self,
        kind: ArtifactKind,
        platform: ArtifactPlatform,
        architecture: ArtifactArchitecture,
        filename: &str,
    ) -> Result<&ArtifactManifestEntry, ArtifactIntegrityError> {
        self.manifest
            .artifacts
            .iter()
            .find(|entry| {
                entry.kind == kind
                    && entry.platform == platform
                    && entry.architecture == architecture
                    && entry.filename == filename
            })
            .ok_or(ArtifactIntegrityError::ArtifactNotFound)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactIntegrityError {
    ManifestTooLarge,
    SignatureTooLarge,
    InvalidSignatureEncoding,
    InvalidTrustKey,
    UntrustedSigningKey,
    RevokedSigningKey,
    InvalidManifest(&'static str),
    ArtifactNotFound,
    ArtifactTooLarge,
    ArtifactLengthMismatch,
    ArtifactDigestMismatch,
}

impl ArtifactIntegrityError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManifestTooLarge => "artifact_manifest_too_large",
            Self::SignatureTooLarge => "artifact_signature_too_large",
            Self::InvalidSignatureEncoding => "artifact_signature_invalid_encoding",
            Self::InvalidTrustKey => "artifact_trust_key_invalid",
            Self::UntrustedSigningKey => "artifact_signing_key_untrusted",
            Self::RevokedSigningKey => "artifact_signing_key_revoked",
            Self::InvalidManifest(_) => "artifact_manifest_invalid",
            Self::ArtifactNotFound => "artifact_not_in_manifest",
            Self::ArtifactTooLarge => "artifact_too_large",
            Self::ArtifactLengthMismatch => "artifact_length_mismatch",
            Self::ArtifactDigestMismatch => "artifact_digest_mismatch",
        }
    }
}

impl std::fmt::Display for ArtifactIntegrityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(reason) => write!(formatter, "{}: {reason}", self.code()),
            _ => formatter.write_str(self.code()),
        }
    }
}

impl std::error::Error for ArtifactIntegrityError {}

pub fn verify_artifact_manifest(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    expectation: ArtifactManifestExpectation<'_>,
) -> Result<VerifiedArtifactManifest, ArtifactIntegrityError> {
    verify_artifact_manifest_with_keys(
        manifest_bytes,
        signature_bytes,
        expectation,
        EMBEDDED_ARTIFACT_TRUST_KEYS,
    )
}

pub fn verify_artifact_manifest_with_keys(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    expectation: ArtifactManifestExpectation<'_>,
    trusted_keys: &[ArtifactTrustKey<'_>],
) -> Result<VerifiedArtifactManifest, ArtifactIntegrityError> {
    if manifest_bytes.len() > MAX_ARTIFACT_MANIFEST_BYTES {
        return Err(ArtifactIntegrityError::ManifestTooLarge);
    }
    if signature_bytes.len() > MAX_ARTIFACT_SIGNATURE_BYTES {
        return Err(ArtifactIntegrityError::SignatureTooLarge);
    }
    let signature_text = decode_signature_text(signature_bytes)?;
    let signature = Signature::decode(&signature_text)
        .map_err(|_| ArtifactIntegrityError::InvalidSignatureEncoding)?;

    let mut invalid_trust_key = false;
    // Revocation wins when the same underlying public key appears in more than
    // one trust record. Check revoked material before accepting any active
    // record so a stale duplicate cannot bypass an emergency revocation.
    for key in trusted_keys
        .iter()
        .filter(|key| key.status == ArtifactTrustKeyStatus::Revoked)
    {
        let public_key = match PublicKey::from_base64(key.public_key_base64) {
            Ok(key) => key,
            Err(_) => {
                invalid_trust_key = true;
                continue;
            }
        };
        if public_key.verify(manifest_bytes, &signature, false).is_ok() {
            return Err(ArtifactIntegrityError::RevokedSigningKey);
        }
    }

    for key in trusted_keys
        .iter()
        .filter(|key| key.status == ArtifactTrustKeyStatus::Active)
    {
        let public_key = match PublicKey::from_base64(key.public_key_base64) {
            Ok(key) => key,
            Err(_) => {
                invalid_trust_key = true;
                continue;
            }
        };
        if public_key.verify(manifest_bytes, &signature, false).is_ok() {
            let manifest = parse_and_validate_manifest(manifest_bytes, expectation)?;
            return Ok(VerifiedArtifactManifest {
                manifest,
                signing_key_id: key.key_id.to_string(),
            });
        }
    }

    if invalid_trust_key
        && trusted_keys
            .iter()
            .all(|key| PublicKey::from_base64(key.public_key_base64).is_err())
    {
        return Err(ArtifactIntegrityError::InvalidTrustKey);
    }
    Err(ArtifactIntegrityError::UntrustedSigningKey)
}

fn decode_signature_text(signature_bytes: &[u8]) -> Result<String, ArtifactIntegrityError> {
    let direct = std::str::from_utf8(signature_bytes)
        .map_err(|_| ArtifactIntegrityError::InvalidSignatureEncoding)?
        .trim();
    if direct.starts_with("untrusted comment:") {
        return Ok(direct.to_string());
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(direct)
        .map_err(|_| ArtifactIntegrityError::InvalidSignatureEncoding)?;
    if decoded.len() > MAX_ARTIFACT_SIGNATURE_BYTES {
        return Err(ArtifactIntegrityError::SignatureTooLarge);
    }
    let decoded =
        String::from_utf8(decoded).map_err(|_| ArtifactIntegrityError::InvalidSignatureEncoding)?;
    if !decoded.trim().starts_with("untrusted comment:") {
        return Err(ArtifactIntegrityError::InvalidSignatureEncoding);
    }
    Ok(decoded.trim().to_string())
}

fn parse_and_validate_manifest(
    manifest_bytes: &[u8],
    expectation: ArtifactManifestExpectation<'_>,
) -> Result<ArtifactManifest, ArtifactIntegrityError> {
    let manifest: ArtifactManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|_| ArtifactIntegrityError::InvalidManifest("invalid_json"))?;
    if manifest.schema != ARTIFACT_MANIFEST_SCHEMA {
        return Err(ArtifactIntegrityError::InvalidManifest("schema"));
    }
    if manifest.source_repository != expectation.source_repository {
        return Err(ArtifactIntegrityError::InvalidManifest("source_repository"));
    }
    if manifest.release != expectation.release {
        return Err(ArtifactIntegrityError::InvalidManifest("release"));
    }
    if manifest.version != expectation.version || !safe_identifier(&manifest.version, 96) {
        return Err(ArtifactIntegrityError::InvalidManifest("version"));
    }
    if !safe_identifier(&manifest.release, 128) {
        return Err(ArtifactIntegrityError::InvalidManifest("release_format"));
    }
    if chrono::DateTime::parse_from_rfc3339(&manifest.generated_at).is_err() {
        return Err(ArtifactIntegrityError::InvalidManifest("generated_at"));
    }
    if !valid_commit(&manifest.source_commit) {
        return Err(ArtifactIntegrityError::InvalidManifest("source_commit"));
    }
    if manifest.artifacts.is_empty() || manifest.artifacts.len() > MAX_ARTIFACTS {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_count"));
    }

    let mut filenames = HashSet::new();
    let mut targets = HashSet::new();
    for entry in &manifest.artifacts {
        validate_entry(entry, &manifest, expectation)?;
        if !filenames.insert(entry.filename.as_str()) {
            return Err(ArtifactIntegrityError::InvalidManifest(
                "duplicate_filename",
            ));
        }
        if !targets.insert((entry.kind, entry.platform, entry.architecture)) {
            return Err(ArtifactIntegrityError::InvalidManifest("duplicate_target"));
        }
    }
    Ok(manifest)
}

fn validate_entry(
    entry: &ArtifactManifestEntry,
    manifest: &ArtifactManifest,
    expectation: ArtifactManifestExpectation<'_>,
) -> Result<(), ArtifactIntegrityError> {
    if entry.version != manifest.version || entry.version != expectation.version {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_version"));
    }
    if !safe_filename(&entry.filename) {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_filename"));
    }
    if entry.length == 0 || entry.length > MAX_SIGNED_ARTIFACT_BYTES {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_length"));
    }
    if !valid_sha256(&entry.sha256) {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_sha256"));
    }
    if !safe_filename(&entry.sbom.filename)
        || entry.sbom.length == 0
        || entry.sbom.length > MAX_SBOM_BYTES
        || !valid_sha256(&entry.sbom.sha256)
    {
        return Err(ArtifactIntegrityError::InvalidManifest("artifact_sbom"));
    }
    if entry.provenance.source_repository != manifest.source_repository
        || entry.provenance.source_commit != manifest.source_commit
    {
        return Err(ArtifactIntegrityError::InvalidManifest(
            "artifact_provenance_source",
        ));
    }
    if !expectation
        .allowed_workflows
        .contains(&entry.provenance.workflow.as_str())
    {
        return Err(ArtifactIntegrityError::InvalidManifest(
            "artifact_provenance_workflow",
        ));
    }
    let expected_builder = format!(
        "https://github.com/{}/actions/runs/{}",
        manifest.source_repository, entry.provenance.run_id
    );
    if entry.provenance.run_id == 0 || entry.provenance.builder_id != expected_builder {
        return Err(ArtifactIntegrityError::InvalidManifest(
            "artifact_provenance_builder",
        ));
    }
    Ok(())
}

fn safe_identifier(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

fn safe_filename(value: &str) -> bool {
    safe_identifier(value, 180) && value != "." && value != ".."
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone)]
pub struct ArtifactDigestVerifier {
    expected_length: u64,
    expected_sha256: String,
    observed_length: u64,
    hasher: Sha256,
}

impl ArtifactDigestVerifier {
    pub fn new(
        entry: &ArtifactManifestEntry,
        consumer_limit: u64,
    ) -> Result<Self, ArtifactIntegrityError> {
        if entry.length > consumer_limit || entry.length > MAX_SIGNED_ARTIFACT_BYTES {
            return Err(ArtifactIntegrityError::ArtifactTooLarge);
        }
        Ok(Self {
            expected_length: entry.length,
            expected_sha256: entry.sha256.clone(),
            observed_length: 0,
            hasher: Sha256::new(),
        })
    }

    pub fn expected_length(&self) -> u64 {
        self.expected_length
    }

    pub fn observed_length(&self) -> u64 {
        self.observed_length
    }

    pub fn update(&mut self, chunk: &[u8]) -> Result<(), ArtifactIntegrityError> {
        self.observed_length = self
            .observed_length
            .checked_add(chunk.len() as u64)
            .ok_or(ArtifactIntegrityError::ArtifactTooLarge)?;
        if self.observed_length > self.expected_length {
            return Err(ArtifactIntegrityError::ArtifactLengthMismatch);
        }
        self.hasher.update(chunk);
        Ok(())
    }

    pub fn finalize(self) -> Result<(), ArtifactIntegrityError> {
        if self.observed_length != self.expected_length {
            return Err(ArtifactIntegrityError::ArtifactLengthMismatch);
        }
        let digest = format!("{:x}", self.hasher.finalize());
        if digest != self.expected_sha256 {
            return Err(ArtifactIntegrityError::ArtifactDigestMismatch);
        }
        Ok(())
    }
}

pub fn verify_artifact_bytes(
    entry: &ArtifactManifestEntry,
    bytes: &[u8],
    consumer_limit: u64,
) -> Result<(), ArtifactIntegrityError> {
    let mut verifier = ArtifactDigestVerifier::new(entry, consumer_limit)?;
    verifier.update(bytes)?;
    verifier.finalize()
}

pub fn current_artifact_platform() -> Result<ArtifactPlatform, ArtifactIntegrityError> {
    if cfg!(target_os = "windows") {
        Ok(ArtifactPlatform::Windows)
    } else if cfg!(target_os = "macos") {
        Ok(ArtifactPlatform::Darwin)
    } else if cfg!(target_os = "linux") {
        Ok(ArtifactPlatform::Linux)
    } else {
        Err(ArtifactIntegrityError::ArtifactNotFound)
    }
}

pub fn current_artifact_architecture() -> Result<ArtifactArchitecture, ArtifactIntegrityError> {
    if cfg!(target_arch = "x86_64") {
        Ok(ArtifactArchitecture::X64)
    } else if cfg!(target_arch = "aarch64") {
        Ok(ArtifactArchitecture::Arm64)
    } else {
        Err(ArtifactIntegrityError::ArtifactNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use blake2::{Blake2b512, Digest as BlakeDigest};
    use ed25519_dalek::{Signer, SigningKey};

    const RELEASE_WORKFLOWS: &[&str] = &[".github/workflows/release.yml"];

    fn expectation<'a>() -> ArtifactManifestExpectation<'a> {
        ArtifactManifestExpectation {
            source_repository: "frumu-ai/tandem",
            release: "v0.7.1",
            version: "0.7.1",
            allowed_workflows: RELEASE_WORKFLOWS,
        }
    }

    fn artifact_bytes() -> &'static [u8] {
        b"verified runtime artifact"
    }

    fn manifest() -> ArtifactManifest {
        let digest = format!("{:x}", Sha256::digest(artifact_bytes()));
        ArtifactManifest {
            schema: ARTIFACT_MANIFEST_SCHEMA.to_string(),
            release: "v0.7.1".to_string(),
            version: "0.7.1".to_string(),
            generated_at: "2026-07-24T00:00:00Z".to_string(),
            source_repository: "frumu-ai/tandem".to_string(),
            source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            artifacts: vec![ArtifactManifestEntry {
                kind: ArtifactKind::Engine,
                version: "0.7.1".to_string(),
                platform: ArtifactPlatform::Linux,
                architecture: ArtifactArchitecture::X64,
                filename: "tandem-engine-linux-x64.tar.gz".to_string(),
                length: artifact_bytes().len() as u64,
                sha256: digest,
                sbom: ArtifactSbom {
                    filename: "tandem-engine-linux-x64.tar.gz.sbom.spdx.json".to_string(),
                    length: 512,
                    sha256: "b".repeat(64),
                },
                provenance: ArtifactProvenance {
                    source_repository: "frumu-ai/tandem".to_string(),
                    source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                    workflow: ".github/workflows/release.yml".to_string(),
                    run_id: 42,
                    builder_id: "https://github.com/frumu-ai/tandem/actions/runs/42".to_string(),
                },
            }],
        }
    }

    fn test_signature(manifest_bytes: &[u8], seed: u8) -> (String, String) {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let key_id = [seed; 8];

        let mut public_record = Vec::with_capacity(42);
        public_record.extend_from_slice(b"Ed");
        public_record.extend_from_slice(&key_id);
        public_record.extend_from_slice(signing_key.verifying_key().as_bytes());

        let prehash = Blake2b512::digest(manifest_bytes);
        let signature = signing_key.sign(&prehash).to_bytes();
        let trusted_comment = "timestamp:1784851200\tfile:tandem-artifacts-v1.json\tprehashed";
        let mut global_input = signature.to_vec();
        global_input.extend_from_slice(trusted_comment.as_bytes());
        let global_signature = signing_key.sign(&global_input).to_bytes();

        let mut signature_record = Vec::with_capacity(74);
        signature_record.extend_from_slice(b"ED");
        signature_record.extend_from_slice(&key_id);
        signature_record.extend_from_slice(&signature);
        let encoded_signature = format!(
            "untrusted comment: signature from test key\n{}\ntrusted comment: {}\n{}",
            STANDARD.encode(signature_record),
            trusted_comment,
            STANDARD.encode(global_signature)
        );
        (STANDARD.encode(public_record), encoded_signature)
    }

    fn verify_test_manifest(
        manifest: &ArtifactManifest,
        status: ArtifactTrustKeyStatus,
    ) -> Result<VerifiedArtifactManifest, ArtifactIntegrityError> {
        let bytes = serde_json::to_vec(manifest).unwrap();
        let (public_key, signature) = test_signature(&bytes, 7);
        let keys = [ArtifactTrustKey {
            key_id: "test-key",
            public_key_base64: &public_key,
            status,
        }];
        verify_artifact_manifest_with_keys(&bytes, signature.as_bytes(), expectation(), &keys)
    }

    #[test]
    fn verifies_signed_manifest_and_exact_target() {
        let verified = verify_test_manifest(&manifest(), ArtifactTrustKeyStatus::Active).unwrap();
        assert_eq!(verified.signing_key_id(), "test-key");
        let artifact = verified
            .artifact(
                ArtifactKind::Engine,
                ArtifactPlatform::Linux,
                ArtifactArchitecture::X64,
                "tandem-engine-linux-x64.tar.gz",
            )
            .unwrap();
        assert_eq!(artifact.version, "0.7.1");
    }

    #[test]
    fn rejects_tampered_manifest_bytes() {
        let original = manifest();
        let bytes = serde_json::to_vec(&original).unwrap();
        let (public_key, signature) = test_signature(&bytes, 7);
        let keys = [ArtifactTrustKey {
            key_id: "test-key",
            public_key_base64: &public_key,
            status: ArtifactTrustKeyStatus::Active,
        }];
        let mut tampered = bytes;
        let position = tampered.iter().position(|byte| *byte == b'7').unwrap();
        tampered[position] = b'8';
        assert_eq!(
            verify_artifact_manifest_with_keys(
                &tampered,
                signature.as_bytes(),
                expectation(),
                &keys,
            )
            .unwrap_err(),
            ArtifactIntegrityError::UntrustedSigningKey
        );
    }

    #[test]
    fn rejects_wrong_release_after_valid_signature() {
        let mut wrong = manifest();
        wrong.release = "v9.9.9".to_string();
        assert_eq!(
            verify_test_manifest(&wrong, ArtifactTrustKeyStatus::Active).unwrap_err(),
            ArtifactIntegrityError::InvalidManifest("release")
        );
    }

    #[test]
    fn rejects_signature_from_revoked_key() {
        assert_eq!(
            verify_test_manifest(&manifest(), ArtifactTrustKeyStatus::Revoked).unwrap_err(),
            ArtifactIntegrityError::RevokedSigningKey
        );
    }

    #[test]
    fn revoked_key_takes_precedence_over_duplicate_active_record() {
        let manifest = manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let (public_key, signature) = test_signature(&bytes, 7);
        let keys = [
            ArtifactTrustKey {
                key_id: "active-copy",
                public_key_base64: &public_key,
                status: ArtifactTrustKeyStatus::Active,
            },
            ArtifactTrustKey {
                key_id: "revoked-copy",
                public_key_base64: &public_key,
                status: ArtifactTrustKeyStatus::Revoked,
            },
        ];
        assert_eq!(
            verify_artifact_manifest_with_keys(&bytes, signature.as_bytes(), expectation(), &keys,)
                .unwrap_err(),
            ArtifactIntegrityError::RevokedSigningKey
        );
    }

    #[test]
    fn rejects_duplicate_targets() {
        let mut duplicate = manifest();
        let mut second = duplicate.artifacts[0].clone();
        second.filename = "other.tar.gz".to_string();
        duplicate.artifacts.push(second);
        assert_eq!(
            verify_test_manifest(&duplicate, ArtifactTrustKeyStatus::Active).unwrap_err(),
            ArtifactIntegrityError::InvalidManifest("duplicate_target")
        );
    }

    #[test]
    fn accepts_tauri_base64_wrapped_minisign_signature() {
        let manifest = manifest();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let (public_key, signature) = test_signature(&bytes, 7);
        let wrapped = STANDARD.encode(signature.as_bytes());
        let keys = [ArtifactTrustKey {
            key_id: "test-key",
            public_key_base64: &public_key,
            status: ArtifactTrustKeyStatus::Active,
        }];
        verify_artifact_manifest_with_keys(&bytes, wrapped.as_bytes(), expectation(), &keys)
            .unwrap();
    }

    #[test]
    fn digest_verifier_accepts_exact_bytes() {
        let entry = &manifest().artifacts[0];
        verify_artifact_bytes(entry, artifact_bytes(), 1024).unwrap();
    }

    #[test]
    fn digest_verifier_rejects_oversize_before_hashing() {
        let entry = &manifest().artifacts[0];
        assert_eq!(
            ArtifactDigestVerifier::new(entry, 4).unwrap_err(),
            ArtifactIntegrityError::ArtifactTooLarge
        );
    }

    #[test]
    fn digest_verifier_rejects_wrong_length_and_digest() {
        let entry = &manifest().artifacts[0];
        assert_eq!(
            verify_artifact_bytes(entry, b"short", 1024),
            Err(ArtifactIntegrityError::ArtifactLengthMismatch)
        );
        let mut wrong = artifact_bytes().to_vec();
        wrong[0] ^= 1;
        assert_eq!(
            verify_artifact_bytes(entry, &wrong, 1024),
            Err(ArtifactIntegrityError::ArtifactDigestMismatch)
        );
    }

    #[test]
    fn embedded_release_key_is_well_formed() {
        for key in EMBEDDED_ARTIFACT_TRUST_KEYS {
            PublicKey::from_base64(key.public_key_base64).unwrap();
        }
    }
}
