use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Minimum PIN length
pub const MIN_PIN_LENGTH: usize = 4;
/// Maximum PIN length
pub const MAX_PIN_LENGTH: usize = 8;

/// Encrypted vault key file format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedVaultKey {
    /// Version for future compatibility
    pub version: u8,
    /// Argon2 salt (22 bytes base64 encoded)
    pub salt: String,
    /// AES-GCM nonce (12 bytes base64 encoded)
    pub nonce: String,
    /// Encrypted master key (32 bytes + 16 byte tag, base64 encoded)
    pub encrypted_key: String,
}

impl EncryptedVaultKey {
    /// Decrypt the master key using a PIN
    pub fn decrypt(&self, pin: &str) -> Result<Vec<u8>> {
        use base64::Engine;

        // Parse salt
        let salt = SaltString::from_b64(&self.salt).map_err(|e| anyhow!("Invalid salt: {}", e))?;

        // Derive key from PIN
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| anyhow!("Failed to hash PIN: {}", e))?;

        let hash_output = password_hash
            .hash
            .ok_or_else(|| anyhow!("No hash output"))?;
        let derived_key: [u8; 32] = hash_output
            .as_bytes()
            .try_into()
            .map_err(|_| anyhow!("Invalid key length"))?;

        // Decode nonce and encrypted key
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.nonce)
            .context("Invalid nonce")?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(&self.encrypted_key)
            .context("Invalid encrypted key")?;

        // Decrypt
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

        let master_key = cipher
            .decrypt(nonce, encrypted.as_slice())
            .map_err(|_| anyhow!("Invalid PIN"))?;

        Ok(master_key)
    }

    /// Load from file
    pub fn load(path: &PathBuf) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .context(format!("Failed to read vault key from {:?}", path))?;
        let vault_key: EncryptedVaultKey =
            serde_json::from_str(&json).context("Failed to parse vault key")?;
        Ok(vault_key)
    }
}

/// Validate PIN format
pub fn validate_pin_format(pin: &str) -> Result<()> {
    if pin.len() < MIN_PIN_LENGTH {
        return Err(anyhow!("PIN must be at least {} digits", MIN_PIN_LENGTH));
    }
    if pin.len() > MAX_PIN_LENGTH {
        return Err(anyhow!("PIN must be at most {} digits", MAX_PIN_LENGTH));
    }
    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow!("PIN must contain only digits"));
    }
    Ok(())
}
