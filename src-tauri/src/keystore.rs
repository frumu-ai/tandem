// Simple encrypted key-value store using the vault's master key

use crate::error::{Result, TandemError};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

/// API key type identifiers for the encrypted keystore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiKeyType {
    OpenRouter,
    OpenCodeZen,
    Anthropic,
    OpenAI,
    Poe,
    Custom(String),
}

impl ApiKeyType {
    pub fn to_key_name(&self) -> String {
        match self {
            ApiKeyType::OpenRouter => "openrouter_key".to_string(),
            ApiKeyType::OpenCodeZen => "opencode_zen_api_key".to_string(),
            ApiKeyType::Anthropic => "anthropic_key".to_string(),
            ApiKeyType::OpenAI => "openai_key".to_string(),
            ApiKeyType::Poe => "poe_api_key".to_string(),
            ApiKeyType::Custom(name) => format!("custom_{}", name),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openrouter" => ApiKeyType::OpenRouter,
            "opencode_zen" | "opencodezen" => ApiKeyType::OpenCodeZen,
            "anthropic" => ApiKeyType::Anthropic,
            "openai" => ApiKeyType::OpenAI,
            "poe" => ApiKeyType::Poe,
            other => ApiKeyType::Custom(other.to_string()),
        }
    }
}

/// Validate that an API key meets basic requirements
pub fn validate_api_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(TandemError::InvalidConfig(
            "API key cannot be empty".to_string(),
        ));
    }

    // Basic validation - keys should be reasonably long
    if key.len() < 10 {
        return Err(TandemError::InvalidConfig(
            "API key appears too short".to_string(),
        ));
    }

    Ok(())
}

/// Validate that a key type is supported
pub fn validate_key_type(key_type: &str) -> Result<ApiKeyType> {
    let api_key_type = ApiKeyType::from_str(key_type);

    // Custom keys need a valid name
    if let ApiKeyType::Custom(name) = &api_key_type {
        if name.is_empty() {
            return Err(TandemError::InvalidConfig(
                "Custom key type requires a name".to_string(),
            ));
        }
    }

    Ok(api_key_type)
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedStore {
    /// Encrypted entries: key -> (nonce, ciphertext)
    entries: HashMap<String, (Vec<u8>, Vec<u8>)>,
}

pub struct SecureKeyStore {
    master_key: Vec<u8>,
    store: RwLock<EncryptedStore>,
    path: std::path::PathBuf,
}

impl SecureKeyStore {
    pub fn new(path: impl AsRef<Path>, master_key: Vec<u8>) -> Result<Self> {
        let store = if path.as_ref().exists() {
            // Load existing store
            let data = std::fs::read(path.as_ref())?;
            serde_json::from_slice(&data)
                .map_err(|e| TandemError::Vault(format!("Failed to parse key store: {}", e)))?
        } else {
            // Create new store
            EncryptedStore {
                entries: HashMap::new(),
            }
        };

        Ok(Self {
            master_key,
            store: RwLock::new(store),
            path: path.as_ref().to_path_buf(),
        })
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| TandemError::Vault(format!("Invalid master key: {}", e)))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt value
        let ciphertext = cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| TandemError::Vault(format!("Encryption failed: {}", e)))?;

        // Store
        let mut store = self.store.write().unwrap();
        store
            .entries
            .insert(key.to_string(), (nonce_bytes.to_vec(), ciphertext));

        // Persist to disk
        self.save_to_disk(&store)?;

        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let store = self.store.read().unwrap();

        let Some((nonce_bytes, ciphertext)) = store.entries.get(key) else {
            return Ok(None);
        };

        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| TandemError::Vault(format!("Invalid master key: {}", e)))?;

        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| TandemError::Vault(format!("Decryption failed: {}", e)))?;

        let value = String::from_utf8(plaintext)
            .map_err(|e| TandemError::Vault(format!("Invalid UTF-8: {}", e)))?;

        Ok(Some(value))
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let mut store = self.store.write().unwrap();
        store.entries.remove(key);
        self.save_to_disk(&store)?;
        Ok(())
    }

    pub fn has(&self, key: &str) -> bool {
        let store = self.store.read().unwrap();
        store.entries.contains_key(key)
    }

    fn save_to_disk(&self, store: &EncryptedStore) -> Result<()> {
        let json = serde_json::to_vec(store)
            .map_err(|e| TandemError::Vault(format!("Failed to serialize store: {}", e)))?;

        std::fs::write(&self.path, json)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_type_conversion() {
        assert!(matches!(
            ApiKeyType::from_str("openrouter"),
            ApiKeyType::OpenRouter
        ));
        assert!(matches!(
            ApiKeyType::from_str("opencode_zen"),
            ApiKeyType::OpenCodeZen
        ));
        assert!(matches!(
            ApiKeyType::from_str("opencodezen"),
            ApiKeyType::OpenCodeZen
        ));
        assert!(matches!(
            ApiKeyType::from_str("anthropic"),
            ApiKeyType::Anthropic
        ));
        assert!(matches!(ApiKeyType::from_str("openai"), ApiKeyType::OpenAI));
        assert!(matches!(ApiKeyType::from_str("poe"), ApiKeyType::Poe));

        if let ApiKeyType::Custom(name) = ApiKeyType::from_str("my_provider") {
            assert_eq!(name, "my_provider");
        } else {
            panic!("Expected Custom variant");
        }
    }

    #[test]
    fn test_validate_api_key() {
        assert!(validate_api_key("sk-1234567890abcdef").is_ok());
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("short").is_err());
    }
}
