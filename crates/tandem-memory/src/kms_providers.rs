use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::decrypt_broker::{
    MemoryDecryptBrokerConfig, MemoryDecryptPurpose, MemoryDekUnwrapProvider,
    MemoryDekUnwrapProviderBox, MemoryDekUnwrapTicket, MemorySecretFamily,
};
use crate::types::{MemoryError, MemoryResult};

pub const GOOGLE_CLOUD_KMS_PROVIDER_ID: &str = "google_cloud_kms";
const GOOGLE_KMS_DECRYPT_COMMAND_ENV: &str = "TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND";
const MEMORY_DEK_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleCloudKmsDecryptRequest {
    pub crypto_key_id: String,
    pub ciphertext: Vec<u8>,
    pub additional_authenticated_data: Vec<u8>,
    pub runtime_principal_id: String,
    pub principal_id: String,
    pub purpose: MemoryDecryptPurpose,
    pub key_scope_id: String,
    pub audit_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct GoogleCloudKmsCommandRequest {
    crypto_key_id: String,
    ciphertext_base64: String,
    additional_authenticated_data_base64: String,
    runtime_principal_id: String,
    principal_id: String,
    purpose: MemoryDecryptPurpose,
    key_scope_id: String,
    audit_id: String,
}

pub trait GoogleCloudKmsDecryptClient {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>>;
}

#[derive(Debug, Clone)]
pub struct GoogleCloudKmsDekUnwrapProvider<C> {
    client: C,
    runtime_principal_id: String,
}

impl<C> GoogleCloudKmsDekUnwrapProvider<C> {
    pub fn new(client: C, runtime_principal_id: impl Into<String>) -> MemoryResult<Self> {
        let runtime_principal_id = runtime_principal_id.into();
        if is_wildcard_or_blank(&runtime_principal_id) {
            return Err(MemoryError::InvalidConfig(
                "google cloud kms memory provider requires a scoped runtime principal".to_string(),
            ));
        }
        Ok(Self {
            client,
            runtime_principal_id,
        })
    }
}

impl<C> MemoryDekUnwrapProvider for GoogleCloudKmsDekUnwrapProvider<C>
where
    C: GoogleCloudKmsDecryptClient + Send + Sync,
{
    fn provider_id(&self) -> &str {
        GOOGLE_CLOUD_KMS_PROVIDER_ID
    }

    fn secret_family(&self) -> MemorySecretFamily {
        MemorySecretFamily::MemoryEnvelope
    }

    fn unwrap_dek(&self, ticket: &MemoryDekUnwrapTicket) -> MemoryResult<Vec<u8>> {
        if !provider_is_google_cloud_kms(&ticket.provider) {
            return Err(MemoryError::InvalidConfig(format!(
                "google cloud kms provider cannot unwrap provider `{}`",
                ticket.provider
            )));
        }
        if ticket.runtime_principal_id != self.runtime_principal_id {
            return Err(MemoryError::InvalidConfig(
                "memory KMS runtime principal does not match configured provider principal"
                    .to_string(),
            ));
        }
        validate_google_cloud_kms_key_id(&ticket.kek_id)?;
        if is_wildcard_or_blank(&ticket.kek_version) {
            return Err(MemoryError::InvalidConfig(
                "google cloud kms ticket requires an explicit key version".to_string(),
            ));
        }
        let ciphertext = decode_wrapped_dek(&ticket.wrapped_dek)?;
        let plaintext = self.client.decrypt(&GoogleCloudKmsDecryptRequest {
            crypto_key_id: ticket.kek_id.clone(),
            ciphertext,
            additional_authenticated_data: ticket.encryption_context_hash.as_bytes().to_vec(),
            runtime_principal_id: ticket.runtime_principal_id.clone(),
            principal_id: ticket.principal_id.clone(),
            purpose: ticket.purpose,
            key_scope_id: ticket.key_scope_id.clone(),
            audit_id: ticket.audit_id.clone(),
        })?;
        if plaintext.len() != MEMORY_DEK_LEN {
            return Err(MemoryError::InvalidConfig(format!(
                "google cloud kms returned {}-byte memory DEK; expected {MEMORY_DEK_LEN}",
                plaintext.len()
            )));
        }
        Ok(plaintext)
    }
}

#[derive(Debug, Clone)]
pub struct GoogleCloudKmsExternalCommandClient {
    command_path: PathBuf,
}

impl GoogleCloudKmsExternalCommandClient {
    pub fn new(command_path: impl Into<PathBuf>) -> MemoryResult<Self> {
        let command_path = command_path.into();
        if command_path.as_os_str().is_empty() {
            return Err(MemoryError::InvalidConfig(
                "google cloud kms decrypt command path is required".to_string(),
            ));
        }
        Ok(Self { command_path })
    }

    pub fn from_env() -> MemoryResult<Option<Self>> {
        let Ok(raw) = std::env::var(GOOGLE_KMS_DECRYPT_COMMAND_ENV) else {
            return Ok(None);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Self::new(trimmed).map(Some)
    }

    pub fn command_path(&self) -> &Path {
        &self.command_path
    }
}

impl GoogleCloudKmsDecryptClient for GoogleCloudKmsExternalCommandClient {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        let command_request = GoogleCloudKmsCommandRequest {
            crypto_key_id: request.crypto_key_id.clone(),
            ciphertext_base64: base64::engine::general_purpose::STANDARD
                .encode(&request.ciphertext),
            additional_authenticated_data_base64: base64::engine::general_purpose::STANDARD
                .encode(&request.additional_authenticated_data),
            runtime_principal_id: request.runtime_principal_id.clone(),
            principal_id: request.principal_id.clone(),
            purpose: request.purpose,
            key_scope_id: request.key_scope_id.clone(),
            audit_id: request.audit_id.clone(),
        };
        let input = serde_json::to_vec(&command_request)?;
        let mut child = Command::new(&self.command_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                MemoryError::InvalidConfig(format!(
                    "failed to spawn google cloud kms decrypt command `{}`: {err}",
                    self.command_path.display()
                ))
            })?;
        let mut stdin = child.stdin.take().ok_or_else(|| {
            MemoryError::InvalidConfig(
                "google cloud kms decrypt command did not expose stdin".to_string(),
            )
        })?;
        stdin.write_all(&input).map_err(|err| {
            MemoryError::InvalidConfig(format!(
                "failed to write google cloud kms decrypt request: {err}"
            ))
        })?;
        drop(stdin);
        let output = child.wait_with_output().map_err(|err| {
            MemoryError::InvalidConfig(format!(
                "failed to wait for google cloud kms decrypt command: {err}"
            ))
        })?;
        if !output.status.success() {
            return Err(MemoryError::InvalidConfig(format!(
                "google cloud kms decrypt command exited with status {}",
                output.status
            )));
        }
        decode_plaintext_dek_output(&output.stdout)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UnconfiguredGoogleCloudKmsClient;

impl GoogleCloudKmsDecryptClient for UnconfiguredGoogleCloudKmsClient {
    fn decrypt(&self, _request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        Err(MemoryError::InvalidConfig(
            "google cloud kms memory decrypt provider is configured, but no decrypt client command is available"
                .to_string(),
        ))
    }
}

pub fn provider_is_google_cloud_kms(provider: &str) -> bool {
    matches!(
        normalize_provider_id(provider).as_str(),
        GOOGLE_CLOUD_KMS_PROVIDER_ID | "google_kms" | "gcp_kms"
    )
}

pub fn memory_dek_unwrap_provider_from_config(
    config: &MemoryDecryptBrokerConfig,
) -> MemoryResult<Option<MemoryDekUnwrapProviderBox>> {
    if !config.hosted_required && provider_is_local(&config.provider) {
        return Ok(None);
    }
    config.validate()?;
    if provider_is_google_cloud_kms(&config.provider) {
        if let Some(client) = GoogleCloudKmsExternalCommandClient::from_env()? {
            return Ok(Some(Box::new(GoogleCloudKmsDekUnwrapProvider::new(
                client,
                config.runtime_principal_id.clone(),
            )?)));
        }
        return Ok(Some(Box::new(GoogleCloudKmsDekUnwrapProvider::new(
            UnconfiguredGoogleCloudKmsClient,
            config.runtime_principal_id.clone(),
        )?)));
    }
    Err(MemoryError::InvalidConfig(format!(
        "unsupported hosted memory decrypt provider `{}`",
        config.provider
    )))
}

fn decode_plaintext_dek_output(output: &[u8]) -> MemoryResult<Vec<u8>> {
    let trimmed = String::from_utf8_lossy(output).trim().to_string();
    if trimmed.is_empty() {
        return Err(MemoryError::InvalidConfig(
            "google cloud kms decrypt command returned empty output".to_string(),
        ));
    }
    if let Ok(value) = serde_json::from_str::<Value>(&trimmed) {
        for key in ["plaintext", "plaintext_base64", "dek", "dek_base64"] {
            if let Some(encoded) = value.get(key).and_then(Value::as_str) {
                return decode_dek_bytes(encoded);
            }
        }
        return Err(MemoryError::InvalidConfig(
            "google cloud kms decrypt command JSON must include plaintext_base64".to_string(),
        ));
    }
    decode_dek_bytes(&trimmed)
}

fn decode_wrapped_dek(raw: &str) -> MemoryResult<Vec<u8>> {
    decode_dek_bytes(raw).map_err(|_| {
        MemoryError::InvalidConfig(
            "memory envelope wrapped_dek must be base64/base64url or hex encoded".to_string(),
        )
    })
}

fn decode_dek_bytes(raw: &str) -> MemoryResult<Vec<u8>> {
    let trimmed = raw.trim();
    if let Some(decoded) = decode_hex(trimmed) {
        return Ok(decoded);
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(trimmed))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .map_err(|_| MemoryError::InvalidConfig("memory DEK encoding is invalid".to_string()))
}

fn validate_google_cloud_kms_key_id(kek_id: &str) -> MemoryResult<()> {
    let value = kek_id.trim();
    if is_wildcard_or_blank(value) {
        return Err(MemoryError::InvalidConfig(
            "google cloud kms key id must be explicit".to_string(),
        ));
    }
    let parts: Vec<&str> = value.split('/').collect();
    if parts.len() != 8
        || parts[0] != "projects"
        || parts[2] != "locations"
        || parts[4] != "keyRings"
        || parts[6] != "cryptoKeys"
        || parts.iter().any(|part| part.trim().is_empty())
    {
        return Err(MemoryError::InvalidConfig(
            "google cloud kms key id must be a full projects/*/locations/*/keyRings/*/cryptoKeys/* resource".to_string(),
        ));
    }
    Ok(())
}

fn provider_is_local(provider: &str) -> bool {
    let normalized = normalize_provider_id(provider);
    normalized.is_empty() || normalized == "disabled" || normalized.starts_with("local")
}

fn normalize_provider_id(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', '.'], "_")
}

fn is_wildcard_or_blank(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "*" | "all" | "global" | "default"
    )
}

fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    let raw = raw.trim();
    if raw.is_empty() || !raw.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(raw.len() / 2);
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let hi = (bytes[index] as char).to_digit(16)?;
        let lo = (bytes[index + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        index += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decrypt_broker::MemoryDekUnwrapTicket;

    #[derive(Debug, Clone)]
    struct FixtureGoogleKmsClient;

    impl GoogleCloudKmsDecryptClient for FixtureGoogleKmsClient {
        fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
            assert_eq!(
                request.crypto_key_id,
                "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance"
            );
            assert_eq!(request.runtime_principal_id, "runtime-memory-decryptor");
            assert_eq!(
                request.key_scope_id,
                "tandem/memory/acme/finance/prod/internal"
            );
            assert_eq!(request.additional_authenticated_data, b"ctx-hash");
            Ok(vec![7u8; MEMORY_DEK_LEN])
        }
    }

    #[derive(Debug, Clone)]
    struct HexWrappedDekGoogleKmsClient;

    impl GoogleCloudKmsDecryptClient for HexWrappedDekGoogleKmsClient {
        fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
            assert_eq!(request.ciphertext, vec![0xaau8; MEMORY_DEK_LEN]);
            Ok(vec![8u8; MEMORY_DEK_LEN])
        }
    }

    fn ticket() -> MemoryDekUnwrapTicket {
        MemoryDekUnwrapTicket {
            provider: "google_cloud_kms".to_string(),
            runtime_principal_id: "runtime-memory-decryptor".to_string(),
            principal_id: "kb-mcp-retrieval-gateway".to_string(),
            purpose: MemoryDecryptPurpose::RetrievalGateway,
            key_scope_id: "tandem/memory/acme/finance/prod/internal".to_string(),
            kek_id: "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance".to_string(),
            kek_version: "1".to_string(),
            wrapped_dek: base64::engine::general_purpose::STANDARD.encode(b"wrapped-dek"),
            algorithm: "AES-256-GCM".to_string(),
            encryption_context_hash: "ctx-hash".to_string(),
            policy_decision_id: "decision-1".to_string(),
            audit_id: "audit-1".to_string(),
            key_lifecycle_decision: None,
        }
    }

    #[test]
    fn google_kms_provider_unwraps_authorized_ticket_through_client() {
        let provider = GoogleCloudKmsDekUnwrapProvider::new(
            FixtureGoogleKmsClient,
            "runtime-memory-decryptor",
        )
        .expect("provider");
        assert_eq!(provider.provider_id(), GOOGLE_CLOUD_KMS_PROVIDER_ID);
        assert_eq!(provider.secret_family(), MemorySecretFamily::MemoryEnvelope);
        assert_eq!(
            provider.unwrap_dek(&ticket()).unwrap(),
            vec![7u8; MEMORY_DEK_LEN]
        );
    }

    #[test]
    fn google_kms_provider_decodes_ambiguous_hex_wrapped_dek_as_hex() {
        let provider = GoogleCloudKmsDekUnwrapProvider::new(
            HexWrappedDekGoogleKmsClient,
            "runtime-memory-decryptor",
        )
        .expect("provider");
        let mut ticket = ticket();
        ticket.wrapped_dek = "aa".repeat(MEMORY_DEK_LEN);
        assert_eq!(
            provider.unwrap_dek(&ticket).unwrap(),
            vec![8u8; MEMORY_DEK_LEN]
        );
    }

    #[test]
    fn google_kms_provider_rejects_wrong_runtime_principal() {
        let provider = GoogleCloudKmsDekUnwrapProvider::new(
            FixtureGoogleKmsClient,
            "runtime-memory-decryptor",
        )
        .expect("provider");
        let mut ticket = ticket();
        ticket.runtime_principal_id = "broad-runtime".to_string();
        let err = provider
            .unwrap_dek(&ticket)
            .expect_err("principal mismatch");
        assert!(err.to_string().contains("runtime principal"));
    }

    #[test]
    fn google_kms_provider_rejects_versioned_key_resource() {
        let provider = GoogleCloudKmsDekUnwrapProvider::new(
            FixtureGoogleKmsClient,
            "runtime-memory-decryptor",
        )
        .expect("provider");
        let mut ticket = ticket();
        ticket.kek_id.push_str("/cryptoKeyVersions/1");
        let err = provider.unwrap_dek(&ticket).expect_err("versioned key");
        assert!(err
            .to_string()
            .contains("full projects/*/locations/*/keyRings/*/cryptoKeys/* resource"));
    }

    #[test]
    fn google_kms_provider_rejects_malformed_key_resource() {
        let provider = GoogleCloudKmsDekUnwrapProvider::new(
            FixtureGoogleKmsClient,
            "runtime-memory-decryptor",
        )
        .expect("provider");
        let mut ticket = ticket();
        ticket.kek_id = "projects/acme/cryptoKeys/finance/locations/global".to_string();
        let err = provider.unwrap_dek(&ticket).expect_err("malformed key");
        assert!(err
            .to_string()
            .contains("full projects/*/locations/*/keyRings/*/cryptoKeys/* resource"));
    }

    #[test]
    fn provider_factory_keeps_local_mode_disabled() {
        let config = MemoryDecryptBrokerConfig::local_disabled();
        assert!(memory_dek_unwrap_provider_from_config(&config)
            .unwrap()
            .is_none());
    }

    #[test]
    fn provider_factory_instantiates_google_provider_from_hosted_config() {
        let config =
            MemoryDecryptBrokerConfig::hosted("google-cloud-kms", "runtime-memory-decryptor")
                .expect("hosted config");
        let provider = memory_dek_unwrap_provider_from_config(&config)
            .expect("factory")
            .expect("provider");
        assert_eq!(provider.provider_id(), GOOGLE_CLOUD_KMS_PROVIDER_ID);
        assert_eq!(provider.secret_family(), MemorySecretFamily::MemoryEnvelope);
    }

    #[test]
    fn command_output_accepts_json_plaintext_base64() {
        let encoded = base64::engine::general_purpose::STANDARD.encode([9u8; MEMORY_DEK_LEN]);
        let out = format!(r#"{{"plaintext_base64":"{encoded}"}}"#);
        assert_eq!(
            decode_plaintext_dek_output(out.as_bytes()).unwrap(),
            vec![9u8; MEMORY_DEK_LEN]
        );
    }

    #[test]
    fn command_output_decodes_ambiguous_hex_plaintext_as_hex() {
        let out = "aa".repeat(MEMORY_DEK_LEN);
        assert_eq!(
            decode_plaintext_dek_output(out.as_bytes()).unwrap(),
            vec![0xaau8; MEMORY_DEK_LEN]
        );
    }
}
