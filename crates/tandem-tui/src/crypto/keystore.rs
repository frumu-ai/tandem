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

    pub fn is_empty_on_disk(path: impl AsRef<Path>) -> Result<bool> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(true);
        }
        let data = std::fs::read(path)?;
        let store: EncryptedStore =
            serde_json::from_slice(&data).context("Failed to parse key store")?;
        Ok(store.entries.is_empty())
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
        let path = path.as_ref();
        let data = serde_json::to_vec_pretty(&self.store).context("Failed to encode key store")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create key store parent {:?}", parent))?;
        }
        write_secret_file(path, &data)
            .context(format!("Failed to write key store to {:?}", path))?;
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

fn write_secret_file(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)?;
    }

    Ok(())
}
