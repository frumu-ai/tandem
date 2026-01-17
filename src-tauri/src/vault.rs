// Tandem Vault - PIN-based encryption for the Stronghold master key
//
// Security model:
// 1. On first run, generate a random 32-byte master key
// 2. User provides a 4-6 digit PIN
// 3. Derive an encryption key from PIN using Argon2id
// 4. Encrypt the master key with AES-256-GCM
// 5. Store: salt + nonce + encrypted_master_key in vault.key file
// 6. On subsequent runs, user enters PIN to decrypt master key

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{Result, TandemError};

/// Minimum PIN length
pub const MIN_PIN_LENGTH: usize = 4;
/// Maximum PIN length  
pub const MAX_PIN_LENGTH: usize = 8;

/// Vault status returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultStatus {
    /// No vault exists - first time setup needed
    NotCreated,
    /// Vault exists but is locked - PIN required
    Locked,
    /// Vault is unlocked and ready
    Unlocked,
}

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
    /// Create a new encrypted vault key from a PIN and random master key
    pub fn create(pin: &str) -> Result<(Self, Vec<u8>)> {
        // Generate random master key (32 bytes for AES-256)
        let mut master_key = vec![0u8; 32];
        OsRng.fill_bytes(&mut master_key);

        // Generate salt for Argon2
        let salt = SaltString::generate(&mut OsRng);

        // Derive encryption key from PIN using Argon2id
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| TandemError::Vault(format!("Failed to hash PIN: {}", e)))?;

        // Extract the hash output (32 bytes)
        let hash_output = password_hash
            .hash
            .ok_or_else(|| TandemError::Vault("No hash output".to_string()))?;
        let derived_key: [u8; 32] = hash_output
            .as_bytes()
            .try_into()
            .map_err(|_| TandemError::Vault("Invalid key length".to_string()))?;

        // Generate nonce for AES-GCM
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt master key with AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| TandemError::Vault(format!("Failed to create cipher: {}", e)))?;

        let encrypted = cipher
            .encrypt(nonce, master_key.as_slice())
            .map_err(|e| TandemError::Vault(format!("Failed to encrypt master key: {}", e)))?;

        let vault_key = EncryptedVaultKey {
            version: 1,
            salt: salt.to_string(),
            nonce: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &nonce_bytes),
            encrypted_key: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &encrypted,
            ),
        };

        Ok((vault_key, master_key))
    }

    /// Decrypt the master key using a PIN
    pub fn decrypt(&self, pin: &str) -> Result<Vec<u8>> {
        use base64::Engine;

        // Parse salt
        let salt = SaltString::from_b64(&self.salt)
            .map_err(|e| TandemError::Vault(format!("Invalid salt: {}", e)))?;

        // Derive key from PIN
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| TandemError::Vault(format!("Failed to hash PIN: {}", e)))?;

        let hash_output = password_hash
            .hash
            .ok_or_else(|| TandemError::Vault("No hash output".to_string()))?;
        let derived_key: [u8; 32] = hash_output
            .as_bytes()
            .try_into()
            .map_err(|_| TandemError::Vault("Invalid key length".to_string()))?;

        // Decode nonce and encrypted key
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.nonce)
            .map_err(|e| TandemError::Vault(format!("Invalid nonce: {}", e)))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(&self.encrypted_key)
            .map_err(|e| TandemError::Vault(format!("Invalid encrypted key: {}", e)))?;

        // Decrypt
        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| TandemError::Vault(format!("Failed to create cipher: {}", e)))?;

        let master_key = cipher
            .decrypt(nonce, encrypted.as_slice())
            .map_err(|_| TandemError::Vault("Invalid PIN".to_string()))?;

        Ok(master_key)
    }

    /// Save to file
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| TandemError::Vault(format!("Failed to serialize vault key: {}", e)))?;
        std::fs::write(path, json)
            .map_err(|e| TandemError::Vault(format!("Failed to write vault key: {}", e)))?;
        Ok(())
    }

    /// Load from file
    pub fn load(path: &PathBuf) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| TandemError::Vault(format!("Failed to read vault key: {}", e)))?;
        let vault_key: EncryptedVaultKey = serde_json::from_str(&json)
            .map_err(|e| TandemError::Vault(format!("Failed to parse vault key: {}", e)))?;
        Ok(vault_key)
    }
}

/// Validate PIN format
pub fn validate_pin(pin: &str) -> Result<()> {
    if pin.len() < MIN_PIN_LENGTH {
        return Err(TandemError::Vault(format!(
            "PIN must be at least {} digits",
            MIN_PIN_LENGTH
        )));
    }
    if pin.len() > MAX_PIN_LENGTH {
        return Err(TandemError::Vault(format!(
            "PIN must be at most {} digits",
            MAX_PIN_LENGTH
        )));
    }
    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(TandemError::Vault(
            "PIN must contain only digits".to_string(),
        ));
    }
    Ok(())
}

/// Get the vault key file path
pub fn get_vault_key_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("vault.key")
}

/// Check if vault exists
pub fn vault_exists(app_data_dir: &PathBuf) -> bool {
    get_vault_key_path(app_data_dir).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_validation() {
        assert!(validate_pin("1234").is_ok());
        assert!(validate_pin("123456").is_ok());
        assert!(validate_pin("12345678").is_ok());
        assert!(validate_pin("123").is_err()); // too short
        assert!(validate_pin("123456789").is_err()); // too long
        assert!(validate_pin("12ab").is_err()); // not digits
    }

    #[test]
    fn test_vault_key_roundtrip() {
        let pin = "1234";
        let (vault_key, original_master_key) = EncryptedVaultKey::create(pin).unwrap();

        let decrypted = vault_key.decrypt(pin).unwrap();
        assert_eq!(original_master_key, decrypted);

        // Wrong PIN should fail
        assert!(vault_key.decrypt("9999").is_err());
    }
}
