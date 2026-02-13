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

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let data = serde_json::to_vec_pretty(&self.store).context("Failed to encode key store")?;
        std::fs::write(path.as_ref(), data)
            .context(format!("Failed to write key store to {:?}", path.as_ref()))?;
        Ok(())
    }

    pub fn list_keys(&self) -> Vec<String> {
        self.store.entries.keys().cloned().collect()
    }

    pub fn set(&mut self, key: &str, value: String) -> Result<()> {
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| anyhow!("Invalid master key: {}", e))?;

        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, value.as_bytes())
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        self.store
            .entries
            .insert(key.to_string(), (nonce_bytes.to_vec(), ciphertext));
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> bool {
        self.store.entries.remove(key).is_some()
    }
}
