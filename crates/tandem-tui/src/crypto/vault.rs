use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Minimum passphrase length for protecting the vault master key.
pub const MIN_PIN_LENGTH: usize = 12;
/// Maximum passphrase length accepted by the local vault UI.
pub const MAX_PIN_LENGTH: usize = 128;

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
    /// Create a new encrypted vault key from a passphrase and random master key.
    pub fn create(pin: &str) -> Result<(Self, Vec<u8>)> {
        validate_pin_format(pin)?;

        let mut master_key = vec![0u8; 32];
        OsRng.fill_bytes(&mut master_key);

        let salt = SaltString::generate(&mut OsRng);
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

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&derived_key)
            .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;
        let encrypted = cipher
            .encrypt(nonce, master_key.as_slice())
            .map_err(|e| anyhow!("Failed to encrypt master key: {}", e))?;

        let vault_key = EncryptedVaultKey {
            version: 1,
            salt: salt.to_string(),
            nonce: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, nonce_bytes),
            encrypted_key: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &encrypted,
            ),
        };

        Ok((vault_key, master_key))
    }

    /// Persist vault key to disk.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("Failed to serialize vault key")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create vault key parent {:?}", parent))?;
        }
        write_secret_file(path, json.as_bytes())
            .context(format!("Failed to write vault key to {:?}", path))?;
        Ok(())
    }

    /// Decrypt the master key using a passphrase
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

fn write_secret_file(path: &PathBuf, bytes: &[u8]) -> Result<()> {
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

/// Validate passphrase format.
pub fn validate_pin_format(pin: &str) -> Result<()> {
    if pin.len() < MIN_PIN_LENGTH || pin.len() > MAX_PIN_LENGTH {
        return Err(anyhow!(
            "Passphrase must be between {} and {} characters",
            MIN_PIN_LENGTH,
            MAX_PIN_LENGTH
        ));
    }
    Ok(())
}
