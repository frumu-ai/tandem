// Simple encrypted key-value store using the vault's master key
// This avoids the double Argon2 overhead of Stronghold

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
