use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedStore {
    /// Encrypted entries: key -> (nonce, ciphertext)
    entries: HashMap<String, (Vec<u8>, Vec<u8>)>,
}

pub struct SecureKeyStore {
    master_key: Vec<u8>,
    store: EncryptedStore,
}

impl SecureKeyStore {
    pub fn load(path: impl AsRef<Path>, master_key: Vec<u8>) -> Result<Self> {
        let store = if path.as_ref().exists() {
            // Load existing store
            let data = std::fs::read(path.as_ref())?;
            serde_json::from_slice(&data).context("Failed to parse key store")?
        } else {
            // Create new store (empty)
            EncryptedStore {
                entries: HashMap::new(),
            }
        };

        Ok(Self { master_key, store })
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let Some((nonce_bytes, ciphertext)) = self.store.entries.get(key) else {
            return Ok(None);
        };

        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| anyhow!("Invalid master key: {}", e))?;

        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        let value = String::from_utf8(plaintext).context("Invalid UTF-8")?;

        Ok(Some(value))
    }
}
